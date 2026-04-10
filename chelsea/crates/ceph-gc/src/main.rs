use ceph::{RbdClient, RbdClientError, RbdSnapName};
use vers_config::VersConfig;
use vers_pg::db::VersPg;

mod error;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let dry_run = match std::env::args().nth(1).as_deref() {
        Some("dry-run") => true,
        Some("run") => false,
        _ => anyhow::bail!("Usage: ceph-gc <dry-run|run>"),
    };

    let rbd = ceph::default_rbd_client()?;
    let pg = VersPg::new().await?;

    if dry_run {
        println!("Dry run mode: no deletions will be performed.");
    }

    // Trim leaf snaps then leaf images in a loop.
    loop {
        println!("\nDeleting leaf snaps.");
        let snap_leaves = find_and_delete_leaf_snaps(rbd, &pg, dry_run).await?;
        println!("Leaf snaps deleted: {}", snap_leaves.len());
        let no_snap_leaves = snap_leaves.is_empty();

        println!("\nDeleting leaf images.");
        let image_leaves = find_and_delete_leaf_images(rbd, &pg, dry_run).await?;
        println!("Leaf images deleted: {}", image_leaves.len());
        let no_image_leaves = image_leaves.is_empty();

        if no_snap_leaves && no_image_leaves {
            println!("\nNo leaves remaining. Done.");
            break;
        }

        // A dry run would loop indefinitely since nothing ever gets deleted; break early for this reason.
        if dry_run {
            break;
        }
    }

    Ok(())
}

/// Deletes the provided image if it meets all of the following criteria:
/// 1) No VM in postgres.chelsea.vm references it.
/// 2) It has no active watchers (eg: clients actively mapping the image.)
/// 3) It has a snapshot count of 0.
///
/// Returns true if the image was deleted, false otherwise.
async fn delete_image_if_leaf(
    rbd: &RbdClient,
    pg: &VersPg,
    dry_run: bool,
    image_name: &str,
) -> Result<bool, error::Error> {
    // Skip images with a VM or sleep snapshot referencing them.
    if pg.chelsea.vm.image_name_exists(image_name).await?
        || pg
            .chelsea
            .sleep_snapshot
            .image_name_exists(image_name)
            .await?
    {
        return Ok(false);
    }
    // Skip images that are mapped by an RBD client somewhere.
    if rbd.image_has_watchers(image_name).await? {
        return Ok(false);
    }
    // Skip images with a snapshot count > 0
    let info = rbd.image_info(image_name).await?;
    if info.snapshot_count > 0 {
        return Ok(false);
    }

    // Delete the image (or print it if dry_run == true)
    if dry_run {
        println!("  Would delete {image_name}");
    } else {
        println!("  Deleted {image_name}");
        rbd.image_remove(image_name).await?;
    }

    Ok(true)
}

/// Finds, (optionally) deletes, and returns all leaf images: those with no snapshots and not referenced by any VM or sleep snapshot.
async fn find_and_delete_leaf_images(
    rbd: &RbdClient,
    pg: &VersPg,
    dry_run: bool,
) -> Result<Vec<String>, error::Error> {
    // Create vec of deleted leaves to return
    let mut leaves = Vec::new();

    // Iterate over all Ceph images
    for image_name in rbd.image_list().await? {
        match delete_image_if_leaf(rbd, pg, dry_run, &image_name).await {
            Ok(true) => {
                leaves.push(image_name);
            }
            Ok(false) => (), // Skipped
            Err(e) => match e {
                // Catch "not found" errors
                error::Error::Rbd(RbdClientError::NotFound(e)) => {
                    println!("  Warning: image {image_name}: {e}");
                }
                other => return Err(other.into()),
            },
        }
    }

    // Return deleted leaves
    Ok(leaves)
}

/// Finds, (optionally) deletes, and returns all leaf snapshots: those with no Ceph clone-children and no commit referencing them.
async fn find_and_delete_leaf_snaps(
    rbd: &RbdClient,
    pg: &VersPg,
    dry_run: bool,
) -> Result<Vec<RbdSnapName>, error::Error> {
    // Get a list of Ceph images.
    let images = rbd.image_list().await?;

    // Create vec of deleted snaps to return.
    let mut deleted_snaps = Vec::new();

    // Iterate over Ceph images.
    for image in images {
        // List the image's snaps.
        let snaps = match rbd.snap_list(&image).await {
            Ok(snaps) => snaps,
            Err(e) => match e {
                // Catch "not found" errors.
                RbdClientError::NotFound(e) => {
                    println!("  Warning: image {image}: {e}");
                    continue;
                }
                other => return Err(other.into()),
            },
        };

        // Iterate over the image's snaps.
        for snap in snaps {
            match delete_snap_if_leaf(rbd, pg, dry_run, &snap).await {
                Ok(true) => deleted_snaps.push(snap),
                Ok(false) => (), // Skipped
                // Catch "not found" errors.
                Err(error::Error::Rbd(RbdClientError::NotFound(e))) => {
                    println!("  Warning: snap {snap}: {e}")
                }
                Err(other) => return Err(other),
            }
        }
    }

    Ok(deleted_snaps)
}

/// Deletes the snap if it meets all of the following criteria:
/// 1) Is not a "base image" snap.
/// 2) Does not have children.
/// 3) Is not referenced by a commit.
///
/// Returns true if deleted, false if not.
async fn delete_snap_if_leaf(
    rbd: &RbdClient,
    pg: &VersPg,
    dry_run: bool,
    snap: &RbdSnapName,
) -> Result<bool, crate::error::Error> {
    // Skip the base image snap.
    let base_snap_name = &VersConfig::chelsea().ceph_base_image_snap_name;
    if snap.snap_name == *base_snap_name {
        return Ok(false);
    }
    // Skip snaps that have children.
    if rbd.snap_has_children(&snap).await? {
        return Ok(false);
    }
    // Skip snaps that are referenced by a commit.
    if pg
        .chelsea
        .commit
        .snap_name_exists(&snap.to_string())
        .await?
    {
        return Ok(false);
    }

    // Delete the snap (or print it if dry_run == true)
    if dry_run {
        println!("  Would delete {snap}");
    } else {
        unprotect_if_needed(rbd, &snap).await?;
        rbd.snap_remove(&snap).await?;
        println!("  Deleted {snap}");
    }

    Ok(true)
}

/// Unprotects a snapshot, ignoring the error if it was already unprotected.
async fn unprotect_if_needed(rbd: &RbdClient, snap: &RbdSnapName) -> Result<(), RbdClientError> {
    match rbd.snap_unprotect(snap).await {
        Ok(()) => Ok(()),
        Err(RbdClientError::ExitCode(_, _, ref stderr))
            if stderr.contains("snap is already unprotected") =>
        {
            Ok(())
        }
        Err(e) => Err(e),
    }
}

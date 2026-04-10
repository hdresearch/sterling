use super::download_file::download_file_from_s3;
use crate::{
    checksum::{checksum_extension, with_checksum_extension, write_checksum},
    join_errors,
    s3::{
        compare_checksums,
        error::{DownloadDirectoryError, DownloadDirectoryTaskError},
        list_objects_with_prefix,
    },
};

use anyhow::{anyhow, bail};
use aws_sdk_s3::Client;
use futures::future::join_all;
use std::{collections::HashSet, path::Path, path::PathBuf};
use tokio::fs;
use tracing::debug;

/// Ensure a prefix ends with `/` as required by S3 directory buckets.
fn normalize_prefix(prefix: &str) -> String {
    let mut prefix = prefix.to_string();
    if !prefix.ends_with('/') {
        prefix.push('/');
    }
    prefix
}

/// Given the full set of remote object keys, return only the data-file keys
/// (i.e. those that are not checksum files), after validating that every data
/// file has a matching checksum companion.
///
/// Returns the filtered keys on success, or an error if any data file is
/// missing its checksum companion (integrity violation).
pub fn validate_and_filter_data_keys(object_keys: &HashSet<String>) -> anyhow::Result<Vec<String>> {
    let ext = checksum_extension();
    let mut data_keys = Vec::new();

    for key in object_keys {
        if key.ends_with(ext) {
            continue;
        }

        let checksum_key = format!("{key}.{ext}");
        if !object_keys.contains(&checksum_key) {
            bail!(
                "Remote bucket does not contain checksum key '{checksum_key}' \
                 for file '{key}'; has the upload failed or been tampered with?"
            );
        }

        data_keys.push(key.clone());
    }

    Ok(data_keys)
}

/// The result of checking a single local file against a remote object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChecksumStatus {
    /// The local checksum matches the remote one.
    Match,
    /// The checksums differ.
    Mismatch,
    /// The comparison failed (e.g. I/O error reading local file).
    Error,
}

/// The outcome of classifying a single remote object key against local state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileAction {
    /// Local file is up-to-date — skip download.
    Skip,
    /// File needs to be downloaded (missing locally or checksum mismatch/error).
    Download {
        object_key: String,
        local_path: PathBuf,
    },
}

/// Given a list of validated data keys and their corresponding checksum statuses,
/// decide which files need to be downloaded. Pure decision logic — no I/O.
///
/// `entries` is an iterator of `(object_key, local_exists, checksum_status)`:
/// - `local_exists`: whether the local file already exists on disk
/// - `checksum_status`: `Some(status)` if local file exists and was compared,
///    `None` if the local file doesn't exist (will force download)
pub fn plan_downloads(
    entries: &[(String, PathBuf, bool, Option<ChecksumStatus>)],
) -> Vec<FileAction> {
    entries
        .iter()
        .map(|(object_key, local_path, local_exists, checksum_status)| {
            if !local_exists {
                return FileAction::Download {
                    object_key: object_key.clone(),
                    local_path: local_path.clone(),
                };
            }
            match checksum_status {
                Some(ChecksumStatus::Match) => FileAction::Skip,
                Some(ChecksumStatus::Mismatch) | Some(ChecksumStatus::Error) | None => {
                    FileAction::Download {
                        object_key: object_key.clone(),
                        local_path: local_path.clone(),
                    }
                }
            }
        })
        .collect()
}

pub async fn download_directory_from_s3(
    client: &Client,
    bucket_name: &str,
    prefix: &str,
    out_directory: &Path,
) -> Result<(), DownloadDirectoryError> {
    let prefix = normalize_prefix(prefix);

    // Create the target directory if it doesn't exist
    if !out_directory.exists() {
        fs::create_dir_all(out_directory).await?;
        debug!("Created directory: {:?}", out_directory);
    }

    // Get a list of object keys to be downloaded
    let object_keys = list_objects_with_prefix(client, bucket_name, &prefix).await?;

    // Download all objects in parallel
    let download_tasks = object_keys
        .into_iter()
        .map(|object_key| {
            let bucket = bucket_name.to_string();
            let out_directory = out_directory.to_path_buf();
            let client = client.clone();

            tokio::spawn(async move {
                let file_name = match Path::new(&object_key).file_name() {
                    Some(file_name) => file_name,
                    None => {
                        return Err(DownloadDirectoryTaskError::ExtractFilenameFromKey(
                            object_key,
                        ));
                    }
                };

                let file_path = out_directory.join(file_name);

                debug!("Downloading {} to {:?}", object_key, file_path);
                download_file_from_s3(&client, &bucket, &object_key, &file_path)
                    .await
                    .map_err(Into::into)
            })
        })
        .collect::<Vec<_>>();

    // Wait for all downloads to complete
    let results = join_all(download_tasks).await;

    let errors: Vec<DownloadDirectoryTaskError> = results
        .into_iter()
        .filter_map(|result| match result {
            Ok(result) => result.err(),
            Err(e) => Some(DownloadDirectoryTaskError::from(e)),
        })
        .collect();

    match errors.len() {
        0 => Ok(()),
        _ => Err(DownloadDirectoryError::TaskErrors(errors)),
    }
}

/// Lists all objects from the given S3 directory. For each object, check to ensure that it has a checksum in the same directory, and if so, that it
/// matches the one found on S3. All objects that don't exist or which have diverging checksums will be downloaded.
pub async fn download_from_s3_directory_if_checksums_differ(
    client: &Client,
    bucket_name: &str,
    prefix: &str,
    out_directory: &Path,
) -> anyhow::Result<()> {
    let prefix = normalize_prefix(prefix);

    // List all objects in the S3 directory
    let object_keys: HashSet<String> = list_objects_with_prefix(client, bucket_name, &prefix)
        .await?
        .into_iter()
        .collect();

    // Validate integrity and get data-only keys
    let data_keys = validate_and_filter_data_keys(&object_keys)?;

    // For each data key, gather local state and compare checksums
    let mut entries = Vec::new();
    for object_key in data_keys {
        let file_name = match Path::new(&object_key).file_name() {
            Some(f) => f,
            None => continue,
        };

        let local_file_path = out_directory.join(file_name);
        let local_exists = local_file_path.exists();

        let checksum_status = if local_exists {
            // Ensure local checksum file exists
            let local_checksum_path = with_checksum_extension(&local_file_path);
            if !local_checksum_path.exists() {
                write_checksum(&local_checksum_path, &local_file_path).await?;
            }

            let object_checksum_key = format!("{object_key}.{}", checksum_extension());
            match compare_checksums(
                client,
                bucket_name,
                &local_checksum_path,
                &object_checksum_key,
            )
            .await
            {
                Ok(true) => {
                    debug!("Checksum matches for {:?}", local_file_path);
                    Some(ChecksumStatus::Match)
                }
                Ok(false) => {
                    debug!("Checksum mismatch for {:?}, will download", local_file_path);
                    Some(ChecksumStatus::Mismatch)
                }
                Err(e) => {
                    debug!(
                        "Error comparing checksums for {:?}: {:?}, will download",
                        local_file_path, e
                    );
                    Some(ChecksumStatus::Error)
                }
            }
        } else {
            debug!(
                "Local file {:?} does not exist, will download",
                local_file_path
            );
            None
        };

        entries.push((object_key, local_file_path, local_exists, checksum_status));
    }

    // Use pure planning logic to decide what to download
    let actions = plan_downloads(&entries);

    let files_to_download: Vec<_> = actions
        .into_iter()
        .filter_map(|action| match action {
            FileAction::Download {
                object_key,
                local_path,
            } => Some((object_key, local_path)),
            FileAction::Skip => None,
        })
        .collect();

    // Download all files that need to be downloaded, in parallel
    let download_tasks = files_to_download
        .into_iter()
        .map(|(object_key, local_file_path)| {
            let bucket = bucket_name.to_string();
            let client = client.clone();
            async move {
                debug!(
                    "Downloading {} and its checksum to {:?}",
                    object_key, local_file_path
                );
                download_file_from_s3(&client, &bucket, &object_key, &local_file_path).await?;
                download_file_from_s3(
                    &client,
                    &bucket,
                    &format!("{object_key}.{}", checksum_extension()),
                    with_checksum_extension(local_file_path),
                )
                .await
            }
        });

    let results = join_all(download_tasks).await;

    // Collect errors, if any
    let errors = results
        .into_iter()
        .flat_map(|result| result.err().map(|x| anyhow::Error::from(x)))
        .collect::<Vec<anyhow::Error>>();

    if errors.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(
            "One or more errors downloading from S3 directory: {}",
            join_errors(&errors, "; ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── normalize_prefix ──

    #[test]
    fn normalize_prefix_adds_trailing_slash() {
        assert_eq!(normalize_prefix("dir"), "dir/");
    }

    #[test]
    fn normalize_prefix_preserves_existing_slash() {
        assert_eq!(normalize_prefix("dir/"), "dir/");
    }

    #[test]
    fn normalize_prefix_empty_string() {
        assert_eq!(normalize_prefix(""), "/");
    }

    // ── validate_and_filter_data_keys ──

    #[test]
    fn validate_filters_out_checksum_files() {
        let keys: HashSet<String> = [
            "dir/file.bin".to_string(),
            "dir/file.bin.sha512".to_string(),
        ]
        .into();

        let data = validate_and_filter_data_keys(&keys).unwrap();
        assert_eq!(data, vec!["dir/file.bin"]);
    }

    #[test]
    fn validate_errors_on_missing_checksum() {
        let keys: HashSet<String> = ["dir/file.bin".to_string()].into();

        let result = validate_and_filter_data_keys(&keys);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("does not contain checksum key"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn validate_multiple_files_all_valid() {
        let keys: HashSet<String> = [
            "d/a.bin".to_string(),
            "d/a.bin.sha512".to_string(),
            "d/b.bin".to_string(),
            "d/b.bin.sha512".to_string(),
        ]
        .into();

        let mut data = validate_and_filter_data_keys(&keys).unwrap();
        data.sort();
        assert_eq!(data, vec!["d/a.bin", "d/b.bin"]);
    }

    #[test]
    fn validate_empty_set() {
        let keys: HashSet<String> = HashSet::new();
        let data = validate_and_filter_data_keys(&keys).unwrap();
        assert!(data.is_empty());
    }

    #[test]
    fn validate_only_checksums() {
        let keys: HashSet<String> = ["dir/file.sha512".to_string()].into();
        // A lone .sha512 file has no data companion — it's just skipped (not an error)
        let data = validate_and_filter_data_keys(&keys).unwrap();
        assert!(data.is_empty());
    }

    // ── plan_downloads ──

    #[test]
    fn plan_downloads_missing_file() {
        let entries = vec![(
            "dir/file.bin".to_string(),
            PathBuf::from("/out/file.bin"),
            false,
            None,
        )];

        let actions = plan_downloads(&entries);
        assert_eq!(
            actions,
            vec![FileAction::Download {
                object_key: "dir/file.bin".to_string(),
                local_path: PathBuf::from("/out/file.bin"),
            }]
        );
    }

    #[test]
    fn plan_downloads_matching_checksum_skips() {
        let entries = vec![(
            "dir/file.bin".to_string(),
            PathBuf::from("/out/file.bin"),
            true,
            Some(ChecksumStatus::Match),
        )];

        let actions = plan_downloads(&entries);
        assert_eq!(actions, vec![FileAction::Skip]);
    }

    #[test]
    fn plan_downloads_mismatch_triggers_download() {
        let entries = vec![(
            "dir/file.bin".to_string(),
            PathBuf::from("/out/file.bin"),
            true,
            Some(ChecksumStatus::Mismatch),
        )];

        let actions = plan_downloads(&entries);
        assert_eq!(
            actions,
            vec![FileAction::Download {
                object_key: "dir/file.bin".to_string(),
                local_path: PathBuf::from("/out/file.bin"),
            }]
        );
    }

    #[test]
    fn plan_downloads_error_triggers_download() {
        let entries = vec![(
            "dir/file.bin".to_string(),
            PathBuf::from("/out/file.bin"),
            true,
            Some(ChecksumStatus::Error),
        )];

        let actions = plan_downloads(&entries);
        assert_eq!(
            actions,
            vec![FileAction::Download {
                object_key: "dir/file.bin".to_string(),
                local_path: PathBuf::from("/out/file.bin"),
            }]
        );
    }

    #[test]
    fn plan_downloads_existing_file_no_checksum_status_triggers_download() {
        let entries = vec![(
            "dir/file.bin".to_string(),
            PathBuf::from("/out/file.bin"),
            true,
            None,
        )];

        let actions = plan_downloads(&entries);
        assert_eq!(
            actions,
            vec![FileAction::Download {
                object_key: "dir/file.bin".to_string(),
                local_path: PathBuf::from("/out/file.bin"),
            }]
        );
    }

    #[test]
    fn plan_downloads_mixed_actions() {
        let entries = vec![
            (
                "d/a.bin".to_string(),
                PathBuf::from("/out/a.bin"),
                true,
                Some(ChecksumStatus::Match),
            ),
            (
                "d/b.bin".to_string(),
                PathBuf::from("/out/b.bin"),
                false,
                None,
            ),
            (
                "d/c.bin".to_string(),
                PathBuf::from("/out/c.bin"),
                true,
                Some(ChecksumStatus::Mismatch),
            ),
        ];

        let actions = plan_downloads(&entries);
        assert_eq!(
            actions,
            vec![
                FileAction::Skip,
                FileAction::Download {
                    object_key: "d/b.bin".to_string(),
                    local_path: PathBuf::from("/out/b.bin"),
                },
                FileAction::Download {
                    object_key: "d/c.bin".to_string(),
                    local_path: PathBuf::from("/out/c.bin"),
                },
            ]
        );
    }

    #[test]
    fn plan_downloads_empty_entries() {
        let actions = plan_downloads(&[]);
        assert!(actions.is_empty());
    }
}

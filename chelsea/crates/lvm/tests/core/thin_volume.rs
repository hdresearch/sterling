use std::{ffi::OsStr, sync::Arc};

use lvm::core::{
    BackingFile, BackingFileCreateOptions, LoopDevice, PhysicalVolume, RootVolumeCreateOptions,
    ThinPool, ThinPoolCreateOptions, ThinVolume, VolumeGroup, VolumeGroupCreateOptions,
};

#[tokio::test]
#[ignore]
async fn create_and_delete_thin_volume() {
    let backing_file = BackingFile::new(BackingFileCreateOptions::default())
        .await
        .unwrap();
    let device = LoopDevice::new(backing_file).await.unwrap();
    let volume = PhysicalVolume::new(device).await.unwrap();

    let vg_options = VolumeGroupCreateOptions::default();
    let volume_group = VolumeGroup::new(vec![volume], vg_options).await.unwrap();

    let pool_options = ThinPoolCreateOptions::default();
    let thin_pool = Arc::new(ThinPool::new(volume_group, pool_options).await.unwrap());

    let thin_volume = ThinVolume::new_root(thin_pool, RootVolumeCreateOptions::default())
        .await
        .unwrap();

    assert_eq!(
        thin_volume.path().unwrap().as_os_str(),
        OsStr::new("/dev/chelsea/root")
    );
}

#[tokio::test]
#[ignore]
async fn create_and_delete_snapshot() {
    let backing_file = BackingFile::new(BackingFileCreateOptions::default())
        .await
        .unwrap();
    let device = LoopDevice::new(backing_file).await.unwrap();
    let volume = PhysicalVolume::new(device).await.unwrap();

    let vg_options = VolumeGroupCreateOptions::default();
    let volume_group = VolumeGroup::new(vec![volume], vg_options).await.unwrap();

    let pool_options = ThinPoolCreateOptions::default();
    let thin_pool = Arc::new(ThinPool::new(volume_group, pool_options).await.unwrap());

    let thin_volume = ThinVolume::new_root(thin_pool, RootVolumeCreateOptions::default())
        .await
        .unwrap();

    let snapshot = thin_volume.snapshot(None).await.unwrap();
    let snapshot2 = thin_volume.snapshot(None).await.unwrap();
    let snapshot3 = snapshot2.snapshot(None).await.unwrap();

    assert_eq!(
        snapshot.path().unwrap().as_os_str(),
        OsStr::new("/dev/chelsea/root_snap0")
    );
    assert_eq!(
        snapshot2.path().unwrap().as_os_str(),
        OsStr::new("/dev/chelsea/root_snap1")
    );
    assert_eq!(
        snapshot3.path().unwrap().as_os_str(),
        OsStr::new("/dev/chelsea/root_snap1_snap0")
    );
}

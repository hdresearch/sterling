use lvm::core::{
    BackingFile, BackingFileCreateOptions, LoopDevice, PhysicalVolume, ThinPool,
    ThinPoolCreateOptions, VolumeGroup, VolumeGroupCreateOptions,
};

#[tokio::test]
#[ignore]
pub async fn create_and_delete_thin_pool() {
    let backing_file = BackingFile::new(BackingFileCreateOptions::default())
        .await
        .unwrap();
    let device = LoopDevice::new(backing_file).await.unwrap();
    let volume = PhysicalVolume::new(device).await.unwrap();

    let vg_options = VolumeGroupCreateOptions::default();
    let volume_group = VolumeGroup::new(vec![volume], vg_options).await.unwrap();

    let pool_options = ThinPoolCreateOptions::default();
    let thin_pool = ThinPool::new(volume_group, pool_options).await.unwrap();

    assert_eq!(&thin_pool.path_str(), "/dev/chelsea/pool");
}

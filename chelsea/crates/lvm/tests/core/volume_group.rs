use std::ffi::OsStr;

use lvm::core::{
    BackingFile, BackingFileCreateOptions, LoopDevice, PhysicalVolume, VolumeGroup,
    VolumeGroupCreateOptions,
};

#[tokio::test]
#[ignore]
pub async fn create_and_delete_volume_group() {
    let options = BackingFileCreateOptions {
        filename: "vg-test.img".to_string(),
        ..Default::default()
    };

    let backing_file = BackingFile::new(options).await.unwrap();
    let device = LoopDevice::new(backing_file).await.unwrap();
    let volume = PhysicalVolume::new(device).await.unwrap();

    let vg_options = VolumeGroupCreateOptions::default();
    let volume_group = VolumeGroup::new(vec![volume], vg_options).await.unwrap();

    assert_eq!(volume_group.path().as_os_str(), OsStr::new("/dev/chelsea"));
}

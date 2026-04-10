use lvm::core::{BackingFile, BackingFileCreateOptions, LoopDevice, PhysicalVolume};

#[tokio::test]
#[ignore]
pub async fn create_and_delete_physical_volume() {
    let options = BackingFileCreateOptions {
        filename: "phys-vol-test.img".to_string(),
        ..Default::default()
    };

    let backing_file = BackingFile::new(options).await.unwrap();
    let device = LoopDevice::new(backing_file).await.unwrap();
    let device_path = device.path();
    let physical_volume = PhysicalVolume::new(device).await.unwrap();

    assert_eq!(physical_volume.path(), device_path);
}

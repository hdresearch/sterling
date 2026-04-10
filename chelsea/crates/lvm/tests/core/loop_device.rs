use lvm::core::{BackingFile, BackingFileCreateOptions, LoopDevice};

#[tokio::test]
#[ignore]
pub async fn create_and_delete_loop_device() {
    let options = BackingFileCreateOptions {
        filename: "loop-test.img".to_string(),
        ..Default::default()
    };

    let backing_file = BackingFile::new(options).await.unwrap();
    let loop_device = LoopDevice::new(backing_file).await.unwrap();
    assert!(loop_device
        .path()
        .as_os_str()
        .to_str()
        .unwrap()
        .contains("/dev/loop"));
}

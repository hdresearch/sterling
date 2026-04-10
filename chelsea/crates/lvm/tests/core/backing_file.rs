use std::ffi::OsStr;

use lvm::core::{BackingFile, BackingFileCreateOptions};

#[tokio::test]
#[ignore]
pub async fn create_and_delete_backing_file() {
    let options = BackingFileCreateOptions {
        filename: "backfile-test.img".to_string(),
        ..Default::default()
    };

    let file = BackingFile::new(options).await.unwrap();
    assert_eq!(
        file.path().file_name().unwrap(),
        OsStr::new("backfile-test.img")
    );
}

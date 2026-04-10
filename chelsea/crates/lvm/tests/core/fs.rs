use lvm::core::{LayeredFs, LayeredFsCreateOptions};
use uuid::Uuid;

#[tokio::test]
#[ignore]
pub async fn create_and_delete_layered_fs() {
    let fs = LayeredFs::new(
        LayeredFsCreateOptions::default(),
        Uuid::new_v4().to_string(),
    )
    .await
    .unwrap();
    drop(fs);
}

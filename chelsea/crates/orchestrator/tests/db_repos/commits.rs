use super::harness::*;
use chrono::Utc;
use orchestrator::db::*;
use uuid::Uuid;

#[tokio::test]
async fn test_commit_insert_and_get() {
    let (db, _pg) = setup().await;
    let commit_id = Uuid::new_v4();
    let vm_id = Uuid::new_v4();

    db.vms()
        .insert(
            vm_id,
            None,
            None,
            seed_node_id(),
            "fd00:fe11:deed:1::80".parse().unwrap(),
            "priv".to_string(),
            "pub".to_string(),
            51870,
            seed_api_key_id(),
            Utc::now(),
            None,
            4,
            512,
        )
        .await
        .unwrap();

    let commit = db
        .commits()
        .insert(
            commit_id,
            Some(vm_id),
            None,
            seed_api_key_id(),
            "my-snapshot".to_string(),
            Some("a test commit".to_string()),
            Utc::now(),
            false,
        )
        .await
        .unwrap();

    assert_eq!(commit.id, commit_id);
    assert_eq!(commit.name, "my-snapshot");
    assert_eq!(commit.parent_vm_id, Some(vm_id));

    let fetched = db
        .commits()
        .get_by_id(commit_id)
        .await
        .unwrap()
        .expect("commit should exist");
    assert_eq!(fetched.id, commit_id);
    assert_eq!(fetched.description, Some("a test commit".to_string()));
}

#[tokio::test]
async fn test_commit_get_nonexistent() {
    let (db, _pg) = setup().await;
    let result = db.commits().get_by_id(Uuid::new_v4()).await.unwrap();
    assert!(result.is_none());
}

use super::harness::*;
use orchestrator::db::*;
use uuid::Uuid;

#[tokio::test]
async fn test_org_get_by_id() {
    let (db, _pg) = setup().await;

    let org = db
        .orgs()
        .get_by_id(seed_org_id())
        .await
        .unwrap()
        .expect("seed org should exist");

    assert_eq!(org.id(), seed_org_id());
    assert_eq!(org.account_id(), seed_account_id());
}

#[tokio::test]
async fn test_org_get_by_id_nonexistent() {
    let (db, _pg) = setup().await;
    let result = db.orgs().get_by_id(Uuid::new_v4()).await.unwrap();
    assert!(result.is_none());
}

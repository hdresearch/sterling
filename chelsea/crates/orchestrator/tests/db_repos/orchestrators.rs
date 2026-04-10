use super::harness::*;
use orchestrator::db::*;

#[tokio::test]
async fn test_orchestrator_get_by_region() {
    let (db, _pg) = setup().await;

    let orch = db
        .orchestrator()
        .get_by_region("us-east")
        .await
        .unwrap()
        .expect("seed orchestrator should exist");

    assert_eq!(*orch.id(), seed_orch_id());
}

#[tokio::test]
async fn test_orchestrator_get_by_region_nonexistent() {
    let (db, _pg) = setup().await;
    let result = db.orchestrator().get_by_region("mars-west").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_orchestrator_insert() {
    let (db, _pg) = setup().await;

    let result = db
        .orchestrator()
        .insert(
            "eu-west",
            "10.0.0.1".parse().unwrap(),
            "fake-priv-key".to_string(),
            "fake-pub-key".to_string(),
        )
        .await;

    // Should fail due to wg_ipv6 unique constraint (seed orch already has fd00:fe11:deed:0::ffff)
    assert!(result.is_err(), "expected unique constraint violation");

    let fetched = db
        .orchestrator()
        .get_by_region("us-east")
        .await
        .unwrap()
        .expect("seed orchestrator should still exist");
    assert_eq!(*fetched.id(), seed_orch_id());
}

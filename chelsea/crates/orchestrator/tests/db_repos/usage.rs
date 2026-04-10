use super::harness::*;

#[tokio::test]
async fn test_usage_reporting_state() {
    let (db, _pg) = setup().await;

    let initial = db
        .usage()
        .get_last_reported_interval(&seed_orch_id())
        .await
        .unwrap();
    assert!(initial.is_none());

    db.usage()
        .update_last_reported_interval(&seed_orch_id(), 1000, 2000)
        .await
        .unwrap();

    let fetched = db
        .usage()
        .get_last_reported_interval(&seed_orch_id())
        .await
        .unwrap()
        .expect("should have interval");
    assert_eq!(fetched, (1000, 2000));

    db.usage()
        .update_last_reported_interval(&seed_orch_id(), 2000, 3000)
        .await
        .unwrap();

    let updated = db
        .usage()
        .get_last_reported_interval(&seed_orch_id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated, (2000, 3000));
}

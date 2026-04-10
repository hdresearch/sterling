use super::harness::*;
use orchestrator::db::*;

#[tokio::test]
async fn test_health_check_insert_and_last_5() {
    let (db, _pg) = setup().await;

    for _ in 0..7 {
        db.health()
            .insert(seed_node_id(), NodeStatus::Up, None)
            .await
            .unwrap();
    }

    let last = db.health().last_5(&seed_node_id()).await.unwrap();
    assert_eq!(last.len(), 5);
    assert!(last.iter().all(|hc| hc.status().is_up()));
}

#[tokio::test]
async fn test_health_check_with_telemetry() {
    let (db, _pg) = setup().await;

    let telemetry = HealthCheckTelemetry {
        vcpu_available: Some(48),
        mem_mib_available: Some(96000),
    };

    let hc = db
        .health()
        .insert(seed_node_id(), NodeStatus::Up, Some(telemetry))
        .await
        .unwrap();

    assert_eq!(hc.vcpu_available(), Some(48));
    assert_eq!(hc.mem_mib_available(), Some(96000));
}

#[tokio::test]
async fn test_health_check_delete_by_node() {
    let (db, _pg) = setup().await;

    db.health()
        .insert(seed_node_id(), NodeStatus::Up, None)
        .await
        .unwrap();

    db.health()
        .delete_by_node_id(&seed_node_id())
        .await
        .unwrap();

    let after = db.health().last_5(&seed_node_id()).await.unwrap();
    assert!(after.is_empty());
}

#[tokio::test]
async fn test_node_status_transitions() {
    assert!(NodeStatus::Up.can_change_to(NodeStatus::Down));
    assert!(NodeStatus::Up.can_change_to(NodeStatus::Evicting));
    assert!(!NodeStatus::Up.can_change_to(NodeStatus::Up));
    assert!(NodeStatus::Booting.can_change_to(NodeStatus::Up));
    assert!(NodeStatus::Booting.can_change_to(NodeStatus::Down));
    assert!(!NodeStatus::Evicting.can_change_to(NodeStatus::Up));
    assert!(!NodeStatus::Evicting.can_change_to(NodeStatus::Down));
}

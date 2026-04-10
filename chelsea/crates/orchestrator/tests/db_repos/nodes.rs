use std::net::IpAddr;

use super::harness::*;
use orchestrator::db::*;
use uuid::Uuid;

#[tokio::test]
async fn test_node_get_by_id() {
    let (db, _pg) = setup().await;

    let node = db
        .node()
        .get_by_id(&seed_node_id())
        .await
        .unwrap()
        .expect("seed node should exist");

    assert_eq!(*node.id(), seed_node_id());
    assert_eq!(node.resources().hardware_cpu(), 96);
    assert_eq!(node.resources().hardware_memory_mib(), 193025);
}

#[tokio::test]
async fn test_node_get_by_id_nonexistent() {
    let (db, _pg) = setup().await;
    let result = db.node().get_by_id(&Uuid::new_v4()).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_node_all_under_orchestrator() {
    let (db, _pg) = setup().await;

    let nodes = db
        .node()
        .all_under_orchestrator(&seed_orch_id())
        .await
        .unwrap();

    assert_eq!(nodes.len(), 1);
    assert_eq!(*nodes[0].id(), seed_node_id());
}

#[tokio::test]
async fn test_node_insert_and_delete() {
    let (db, _pg) = setup().await;
    let new_node_id = Uuid::new_v4();

    let resources = NodeResources::new(8, 16384, 100000, 64);

    let node = db
        .node()
        .insert(
            new_node_id,
            &seed_orch_id(),
            &resources,
            "node-priv-key",
            "node-pub-key",
            Some("fd00:fe11:deed:0::99".parse().unwrap()),
            Some("10.0.0.99".parse().unwrap()),
        )
        .await
        .unwrap();

    assert_eq!(*node.id(), new_node_id);
    assert_eq!(node.resources().hardware_cpu(), 8);

    let all = db
        .node()
        .all_under_orchestrator(&seed_orch_id())
        .await
        .unwrap();
    assert_eq!(all.len(), 2);

    db.node().delete(&new_node_id).await.unwrap();
    let after_delete = db.node().get_by_id(&new_node_id).await.unwrap();
    assert!(after_delete.is_none());
}

#[tokio::test]
async fn test_node_set_instance() {
    let (db, _pg) = setup().await;

    db.node()
        .set_node_instance(seed_node_id(), "10.0.0.42".parse().unwrap())
        .await
        .unwrap();

    let node = db.node().get_by_id(&seed_node_id()).await.unwrap().unwrap();
    assert_eq!(node.ip_pub(), "10.0.0.42".parse::<IpAddr>().unwrap());
}

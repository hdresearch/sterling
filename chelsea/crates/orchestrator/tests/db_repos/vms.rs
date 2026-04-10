use std::net::Ipv6Addr;

use super::harness::*;
use chrono::Utc;
use orchestrator::db::*;
use uuid::Uuid;

#[tokio::test]
async fn test_vm_insert_and_get() {
    let (db, _pg) = setup().await;
    let vm_id = Uuid::new_v4();
    let ip: Ipv6Addr = "fd00:fe11:deed:1::10".parse().unwrap();

    let vm = db
        .vms()
        .insert(
            vm_id,
            None,
            None,
            seed_node_id(),
            ip,
            "wg-priv".to_string(),
            "wg-pub".to_string(),
            51830,
            seed_api_key_id(),
            Utc::now(),
            None,
            4,
            512,
        )
        .await
        .unwrap();

    assert_eq!(vm.id(), vm_id);
    assert_eq!(vm.ip, ip);

    let fetched = db.vms().get_by_id(vm_id).await.unwrap().unwrap();
    assert_eq!(fetched.id(), vm_id);
    assert_eq!(fetched.node_id, Some(seed_node_id()));
}

#[tokio::test]
async fn test_vm_list_empty_initially() {
    let (db, _pg) = setup().await;
    let vms = db.vms().list().await.unwrap();
    assert!(vms.is_empty());
}

#[tokio::test]
async fn test_vm_list_and_list_under_node() {
    let (db, _pg) = setup().await;

    for i in 1..=2u8 {
        db.vms()
            .insert(
                Uuid::new_v4(),
                None,
                None,
                seed_node_id(),
                format!("fd00:fe11:deed:1::{i}").parse().unwrap(),
                format!("priv-{i}"),
                format!("pub-{i}"),
                51830 + i as u16,
                seed_api_key_id(),
                Utc::now(),
                None,
                4,
                512,
            )
            .await
            .unwrap();
    }

    let all = db.vms().list().await.unwrap();
    assert_eq!(all.len(), 2);

    let under_node = db.vms().list_under_node(seed_node_id()).await.unwrap();
    assert_eq!(under_node.len(), 2);

    let other = db.vms().list_under_node(Uuid::new_v4()).await.unwrap();
    assert!(other.is_empty());
}

#[tokio::test]
async fn test_vm_mark_deleted() {
    let (db, _pg) = setup().await;
    let vm_id = Uuid::new_v4();

    db.vms()
        .insert(
            vm_id,
            None,
            None,
            seed_node_id(),
            "fd00:fe11:deed:1::20".parse().unwrap(),
            "priv".to_string(),
            "pub".to_string(),
            51835,
            seed_api_key_id(),
            Utc::now(),
            None,
            4,
            512,
        )
        .await
        .unwrap();

    db.vms().mark_deleted(&vm_id).await.unwrap();

    assert!(db.vms().get_by_id(vm_id).await.unwrap().is_none());
    assert!(db.vms().list().await.unwrap().is_empty());
}

#[tokio::test]
async fn test_vm_list_by_api_key() {
    let (db, _pg) = setup().await;
    let vm_id = Uuid::new_v4();

    db.vms()
        .insert(
            vm_id,
            None,
            None,
            seed_node_id(),
            "fd00:fe11:deed:1::30".parse().unwrap(),
            "priv".to_string(),
            "pub".to_string(),
            51840,
            seed_api_key_id(),
            Utc::now(),
            None,
            4,
            512,
        )
        .await
        .unwrap();

    let by_key = db.vms().list_by_api_key(seed_api_key_id()).await.unwrap();
    assert_eq!(by_key.len(), 1);
    assert_eq!(by_key[0].id(), vm_id);

    let by_other = db.vms().list_by_api_key(Uuid::new_v4()).await.unwrap();
    assert!(by_other.is_empty());
}

#[tokio::test]
async fn test_vm_list_by_org_id() {
    let (db, _pg) = setup().await;

    db.vms()
        .insert(
            Uuid::new_v4(),
            None,
            None,
            seed_node_id(),
            "fd00:fe11:deed:1::40".parse().unwrap(),
            "priv".to_string(),
            "pub".to_string(),
            51845,
            seed_api_key_id(),
            Utc::now(),
            None,
            4,
            512,
        )
        .await
        .unwrap();

    let by_org = db.vms().list_by_org_id(seed_org_id()).await.unwrap();
    assert_eq!(by_org.len(), 1);
}

#[tokio::test]
async fn test_vm_grandchild_relationship() {
    let (db, _pg) = setup().await;
    let grandparent_id = Uuid::new_v4();
    let commit_id = Uuid::new_v4();
    let child_id = Uuid::new_v4();

    db.vms()
        .insert(
            grandparent_id,
            None,
            None,
            seed_node_id(),
            "fd00:fe11:deed:1::50".parse().unwrap(),
            "priv-gp".to_string(),
            "pub-gp".to_string(),
            51850,
            seed_api_key_id(),
            Utc::now(),
            None,
            4,
            512,
        )
        .await
        .unwrap();

    db.commits()
        .insert(
            commit_id,
            Some(grandparent_id),
            None,
            seed_api_key_id(),
            "snapshot".to_string(),
            None,
            Utc::now(),
            false,
        )
        .await
        .unwrap();

    db.vms()
        .insert(
            child_id,
            Some(commit_id),
            Some(grandparent_id),
            seed_node_id(),
            "fd00:fe11:deed:1::51".parse().unwrap(),
            "priv-child".to_string(),
            "pub-child".to_string(),
            51851,
            seed_api_key_id(),
            Utc::now(),
            None,
            4,
            512,
        )
        .await
        .unwrap();

    let grandchildren = db.vms().list_grandchild_vms(grandparent_id).await.unwrap();
    assert_eq!(grandchildren.len(), 1);
    assert_eq!(grandchildren[0].id(), child_id);
    assert_eq!(grandchildren[0].parent_commit_id, Some(commit_id));
}

#[tokio::test]
async fn test_vm_wg_port_uniqueness() {
    let (db, _pg) = setup().await;

    db.vms()
        .insert(
            Uuid::new_v4(),
            None,
            None,
            seed_node_id(),
            "fd00:fe11:deed:1::60".parse().unwrap(),
            "priv1".to_string(),
            "pub1".to_string(),
            51860,
            seed_api_key_id(),
            Utc::now(),
            None,
            4,
            512,
        )
        .await
        .unwrap();

    let result = db
        .vms()
        .insert(
            Uuid::new_v4(),
            None,
            None,
            seed_node_id(),
            "fd00:fe11:deed:1::61".parse().unwrap(),
            "priv2".to_string(),
            "pub2".to_string(),
            51860, // same port!
            seed_api_key_id(),
            Utc::now(),
            None,
            4,
            512,
        )
        .await;

    assert!(
        result.is_err(),
        "expected wg_port uniqueness violation, got: {:?}",
        result
    );
}

#[tokio::test]
async fn test_vm_next_wg_port() {
    let (db, _pg) = setup().await;

    let port = db.vms().next_vm_wg_port(seed_node_id()).await.unwrap();
    assert_eq!(port, 51830);

    db.vms()
        .insert(
            Uuid::new_v4(),
            None,
            None,
            seed_node_id(),
            "fd00:fe11:deed:1::70".parse().unwrap(),
            "priv".to_string(),
            "pub".to_string(),
            51830,
            seed_api_key_id(),
            Utc::now(),
            None,
            4,
            512,
        )
        .await
        .unwrap();

    let port2 = db.vms().next_vm_wg_port(seed_node_id()).await.unwrap();
    assert_eq!(port2, 51831);
}

#[tokio::test]
async fn test_vm_allocate_ip() {
    let (db, _pg) = setup().await;

    let ip = db.vms().allocate_vm_ip(seed_account_id()).await.unwrap();
    assert!(ip.to_string().starts_with("fd00:fe11:deed:"));
}

use super::harness::*;
use chrono::Utc;
use orchestrator::db::*;
use uuid::Uuid;

#[tokio::test]
async fn test_api_key_get_by_id() {
    let (db, _pg) = setup().await;

    let key = db
        .keys()
        .get_by_id(seed_api_key_id())
        .await
        .unwrap()
        .expect("seed API key should exist");

    assert_eq!(key.id(), seed_api_key_id());
    assert_eq!(key.org_id(), seed_org_id());
}

#[tokio::test]
async fn test_api_key_get_by_hash() {
    let (db, _pg) = setup().await;

    let key = db
        .keys()
        .get_by_hash(SEED_API_KEY_HASH)
        .await
        .unwrap()
        .expect("seed key should be found by hash");

    assert_eq!(key.id(), seed_api_key_id());
}

#[tokio::test]
async fn test_api_key_get_valid_by_hash() {
    let (db, _pg) = setup().await;

    let key = db
        .keys()
        .get_valid_by_hash(SEED_API_KEY_HASH)
        .await
        .unwrap()
        .expect("seed key should be valid");

    assert_eq!(key.id(), seed_api_key_id());
}

#[tokio::test]
async fn test_api_key_insert_and_revoke() {
    let (db, _pg) = setup().await;

    let key = db
        .keys()
        .insert(
            Uuid::parse_str(SEED_USER_ID).unwrap(),
            seed_org_id(),
            "test-label",
            "PBKDF2",
            100,
            "deadbeef",
            "cafebabe",
            Utc::now(),
            None,
        )
        .await
        .unwrap();

    let valid = db.keys().get_valid_by_hash("cafebabe").await.unwrap();
    assert!(valid.is_some());

    db.keys().revoke(key.id(), Utc::now()).await.unwrap();

    let revoked = db.keys().get_valid_by_hash("cafebabe").await.unwrap();
    assert!(revoked.is_none());

    let still_there = db.keys().get_by_hash("cafebabe").await.unwrap();
    assert!(still_there.is_some());
}

#[tokio::test]
async fn test_api_key_set_deleted() {
    let (db, _pg) = setup().await;

    let key = db
        .keys()
        .insert(
            Uuid::parse_str(SEED_USER_ID).unwrap(),
            seed_org_id(),
            "to-delete",
            "PBKDF2",
            100,
            "salt-del",
            "hash-del",
            Utc::now(),
            None,
        )
        .await
        .unwrap();

    db.keys().set_deleted(key.id(), Utc::now()).await.unwrap();

    let result = db.keys().get_valid_by_hash("hash-del").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_api_key_set_active() {
    let (db, _pg) = setup().await;

    let key = db
        .keys()
        .insert(
            Uuid::parse_str(SEED_USER_ID).unwrap(),
            seed_org_id(),
            "toggle",
            "PBKDF2",
            100,
            "salt-tog",
            "hash-tog",
            Utc::now(),
            None,
        )
        .await
        .unwrap();

    db.keys().set_active(key.id(), false).await.unwrap();
    assert!(
        db.keys()
            .get_valid_by_hash("hash-tog")
            .await
            .unwrap()
            .is_none()
    );

    db.keys().set_active(key.id(), true).await.unwrap();
    assert!(
        db.keys()
            .get_valid_by_hash("hash-tog")
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn test_api_key_list_valid() {
    let (db, _pg) = setup().await;

    let keys = db.keys().list_valid().await.unwrap();
    assert!(!keys.is_empty());
    assert!(keys.iter().any(|k| k.id() == seed_api_key_id()));
}

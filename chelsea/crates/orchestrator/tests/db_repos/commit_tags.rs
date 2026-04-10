use super::harness::*;
use orchestrator::db::*;
use uuid::Uuid;

#[tokio::test]
async fn test_commit_tag_insert_and_get_by_name() {
    let (db, _pg) = setup().await;
    let (_vm_id, commit_id) = create_test_commit(&db, "100").await;

    let tag = db
        .commit_tags()
        .insert(
            "production".to_string(),
            commit_id,
            seed_api_key_id(),
            seed_org_id(),
            Some("The production image".to_string()),
        )
        .await
        .unwrap();

    assert_eq!(tag.tag_name, "production");
    assert_eq!(tag.commit_id, commit_id);
    assert_eq!(tag.owner_id, seed_api_key_id());
    assert_eq!(tag.org_id, seed_org_id());
    assert_eq!(tag.description, Some("The production image".to_string()));

    let fetched = db
        .commit_tags()
        .get_by_name(seed_org_id(), "production")
        .await
        .unwrap()
        .expect("tag should exist");

    assert_eq!(fetched.id, tag.id);
    assert_eq!(fetched.tag_name, "production");
    assert_eq!(fetched.commit_id, commit_id);
}

#[tokio::test]
async fn test_commit_tag_get_by_id() {
    let (db, _pg) = setup().await;
    let (_vm_id, commit_id) = create_test_commit(&db, "101").await;

    let tag = db
        .commit_tags()
        .insert(
            "staging".to_string(),
            commit_id,
            seed_api_key_id(),
            seed_org_id(),
            None,
        )
        .await
        .unwrap();

    let fetched = db
        .commit_tags()
        .get_by_id(tag.id)
        .await
        .unwrap()
        .expect("tag should exist");

    assert_eq!(fetched.tag_name, "staging");
    assert_eq!(fetched.description, None);
}

#[tokio::test]
async fn test_commit_tag_get_nonexistent() {
    let (db, _pg) = setup().await;

    let by_id = db.commit_tags().get_by_id(Uuid::new_v4()).await.unwrap();
    assert!(by_id.is_none());

    let by_name = db
        .commit_tags()
        .get_by_name(seed_org_id(), "nonexistent")
        .await
        .unwrap();
    assert!(by_name.is_none());
}

#[tokio::test]
async fn test_commit_tag_unique_per_org() {
    let (db, _pg) = setup().await;
    let (_vm_id, commit_id) = create_test_commit(&db, "102").await;

    db.commit_tags()
        .insert(
            "release".to_string(),
            commit_id,
            seed_api_key_id(),
            seed_org_id(),
            None,
        )
        .await
        .unwrap();

    let result = db
        .commit_tags()
        .insert(
            "release".to_string(),
            commit_id,
            seed_api_key_id(),
            seed_org_id(),
            None,
        )
        .await;

    assert!(result.is_err(), "expected unique constraint violation");

    let err = result.unwrap_err();
    let db_err = err.as_db_error().expect("should be a DB error");
    assert_eq!(
        db_err.constraint(),
        Some("unique_tag_per_org_legacy"),
        "constraint name should match the partial unique index for legacy org-scoped tags"
    );
}

#[tokio::test]
async fn test_commit_tag_list_by_org() {
    let (db, _pg) = setup().await;
    let (_vm_id, commit_id) = create_test_commit(&db, "103").await;

    let tags = db.commit_tags().list_by_org(seed_org_id()).await.unwrap();
    assert!(tags.is_empty());

    for name in ["alpha", "beta", "gamma"] {
        db.commit_tags()
            .insert(
                name.to_string(),
                commit_id,
                seed_api_key_id(),
                seed_org_id(),
                None,
            )
            .await
            .unwrap();
    }

    let tags = db.commit_tags().list_by_org(seed_org_id()).await.unwrap();
    assert_eq!(tags.len(), 3);

    let names: Vec<&str> = tags.iter().map(|t| t.tag_name.as_str()).collect();
    assert_eq!(names, vec!["alpha", "beta", "gamma"]);

    let other_org = db.commit_tags().list_by_org(Uuid::new_v4()).await.unwrap();
    assert!(other_org.is_empty());
}

#[tokio::test]
async fn test_commit_tag_list_by_commit() {
    let (db, _pg) = setup().await;
    let (_vm1, commit1) = create_test_commit(&db, "104").await;
    let (_vm2, commit2) = create_test_commit(&db, "105").await;

    for (name, cid) in [("tag-a", commit1), ("tag-b", commit1), ("tag-c", commit2)] {
        db.commit_tags()
            .insert(
                name.to_string(),
                cid,
                seed_api_key_id(),
                seed_org_id(),
                None,
            )
            .await
            .unwrap();
    }

    let tags_on_1 = db.commit_tags().list_by_commit(commit1).await.unwrap();
    assert_eq!(tags_on_1.len(), 2);

    let tags_on_2 = db.commit_tags().list_by_commit(commit2).await.unwrap();
    assert_eq!(tags_on_2.len(), 1);
    assert_eq!(tags_on_2[0].tag_name, "tag-c");
}

#[tokio::test]
async fn test_commit_tag_update_commit() {
    let (db, _pg) = setup().await;
    let (_vm1, commit1) = create_test_commit(&db, "106").await;
    let (_vm2, commit2) = create_test_commit(&db, "107").await;

    let tag = db
        .commit_tags()
        .insert(
            "rolling".to_string(),
            commit1,
            seed_api_key_id(),
            seed_org_id(),
            None,
        )
        .await
        .unwrap();

    assert_eq!(tag.commit_id, commit1);

    let updated = db
        .commit_tags()
        .update_commit(tag.id, commit2)
        .await
        .unwrap();

    assert_eq!(updated.id, tag.id);
    assert_eq!(updated.tag_name, "rolling");
    assert_eq!(updated.commit_id, commit2);
    assert!(updated.updated_at > tag.updated_at);

    let fetched = db
        .commit_tags()
        .get_by_name(seed_org_id(), "rolling")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.commit_id, commit2);
}

#[tokio::test]
async fn test_commit_tag_update_description() {
    let (db, _pg) = setup().await;
    let (_vm_id, commit_id) = create_test_commit(&db, "108").await;

    let tag = db
        .commit_tags()
        .insert(
            "desc-test".to_string(),
            commit_id,
            seed_api_key_id(),
            seed_org_id(),
            None,
        )
        .await
        .unwrap();

    assert_eq!(tag.description, None);

    db.commit_tags()
        .update_description(tag.id, Some("now has a description".to_string()))
        .await
        .unwrap();

    let fetched = db.commit_tags().get_by_id(tag.id).await.unwrap().unwrap();
    assert_eq!(
        fetched.description,
        Some("now has a description".to_string())
    );

    db.commit_tags()
        .update_description(tag.id, None)
        .await
        .unwrap();

    let fetched = db.commit_tags().get_by_id(tag.id).await.unwrap().unwrap();
    assert_eq!(fetched.description, None);
}

#[tokio::test]
async fn test_commit_tag_atomic_update() {
    let (db, _pg) = setup().await;
    let (_vm1, commit1) = create_test_commit(&db, "109").await;
    let (_vm2, commit2) = create_test_commit(&db, "110").await;

    let tag = db
        .commit_tags()
        .insert(
            "atomic-test".to_string(),
            commit1,
            seed_api_key_id(),
            seed_org_id(),
            Some("old description".to_string()),
        )
        .await
        .unwrap();

    let updated = db
        .commit_tags()
        .update(
            tag.id,
            Some(commit2),
            Some(Some("new description".to_string())),
        )
        .await
        .unwrap();

    assert_eq!(updated.commit_id, commit2);
    assert_eq!(updated.description, Some("new description".to_string()));
    assert!(updated.updated_at > tag.updated_at);

    let updated2 = db
        .commit_tags()
        .update(updated.id, Some(commit1), None)
        .await
        .unwrap();
    assert_eq!(updated2.commit_id, commit1);
    assert_eq!(updated2.description, Some("new description".to_string()));

    let updated3 = db
        .commit_tags()
        .update(updated2.id, None, Some(None))
        .await
        .unwrap();
    assert_eq!(updated3.commit_id, commit1);
    assert_eq!(updated3.description, None);
}

#[tokio::test]
async fn test_commit_tag_delete() {
    let (db, _pg) = setup().await;
    let (_vm_id, commit_id) = create_test_commit(&db, "111").await;

    let tag = db
        .commit_tags()
        .insert(
            "doomed".to_string(),
            commit_id,
            seed_api_key_id(),
            seed_org_id(),
            None,
        )
        .await
        .unwrap();

    assert!(db.commit_tags().get_by_id(tag.id).await.unwrap().is_some());

    db.commit_tags().delete(tag.id).await.unwrap();

    assert!(db.commit_tags().get_by_id(tag.id).await.unwrap().is_none());
    assert!(
        db.commit_tags()
            .get_by_name(seed_org_id(), "doomed")
            .await
            .unwrap()
            .is_none()
    );

    // Idempotent
    db.commit_tags().delete(tag.id).await.unwrap();
}

#[tokio::test]
async fn test_commit_tag_cascade_on_commit_delete() {
    let (db, _pg) = setup().await;
    let (_vm_id, commit_id) = create_test_commit(&db, "112").await;

    db.commit_tags()
        .insert(
            "cascade-me".to_string(),
            commit_id,
            seed_api_key_id(),
            seed_org_id(),
            None,
        )
        .await
        .unwrap();

    assert!(
        db.commit_tags()
            .get_by_name(seed_org_id(), "cascade-me")
            .await
            .unwrap()
            .is_some()
    );

    db.raw_obj()
        .await
        .execute("DELETE FROM commits WHERE commit_id = $1", &[&commit_id])
        .await
        .unwrap();

    assert!(
        db.commit_tags()
            .get_by_name(seed_org_id(), "cascade-me")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn test_commit_tag_multiple_tags_same_commit() {
    let (db, _pg) = setup().await;
    let (_vm_id, commit_id) = create_test_commit(&db, "113").await;

    for name in ["v1.0", "latest", "stable"] {
        db.commit_tags()
            .insert(
                name.to_string(),
                commit_id,
                seed_api_key_id(),
                seed_org_id(),
                None,
            )
            .await
            .unwrap();
    }

    let tags = db.commit_tags().list_by_commit(commit_id).await.unwrap();
    assert_eq!(tags.len(), 3);

    let names: Vec<&str> = tags.iter().map(|t| t.tag_name.as_str()).collect();
    assert_eq!(names, vec!["latest", "stable", "v1.0"]);
}

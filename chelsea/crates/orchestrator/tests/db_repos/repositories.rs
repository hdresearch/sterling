use super::harness::*;
use orchestrator::db::*;
use uuid::Uuid;

// ── Repository CRUD ─────────────────────────────────────────────────────────

#[tokio::test]
async fn test_repo_insert_and_get_by_name() {
    let (db, _pg) = setup().await;

    let repo = db
        .commit_repositories()
        .insert(
            "myapp".to_string(),
            seed_org_id(),
            seed_api_key_id(),
            Some("My application image".to_string()),
        )
        .await
        .unwrap();

    assert_eq!(repo.name, "myapp");
    assert_eq!(repo.org_id, seed_org_id());
    assert_eq!(repo.owner_id, seed_api_key_id());
    assert_eq!(repo.description, Some("My application image".to_string()));

    let fetched = db
        .commit_repositories()
        .get_by_name(seed_org_id(), "myapp")
        .await
        .unwrap()
        .expect("repo should exist");
    assert_eq!(fetched.id, repo.id);
    assert_eq!(fetched.name, "myapp");
}

#[tokio::test]
async fn test_repo_get_by_id() {
    let (db, _pg) = setup().await;

    let repo = db
        .commit_repositories()
        .insert(
            "base-ubuntu".to_string(),
            seed_org_id(),
            seed_api_key_id(),
            None,
        )
        .await
        .unwrap();

    let fetched = db
        .commit_repositories()
        .get_by_id(repo.id)
        .await
        .unwrap()
        .expect("repo should exist");
    assert_eq!(fetched.name, "base-ubuntu");
    assert_eq!(fetched.description, None);
}

#[tokio::test]
async fn test_repo_get_nonexistent() {
    let (db, _pg) = setup().await;

    assert!(
        db.commit_repositories()
            .get_by_id(Uuid::new_v4())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        db.commit_repositories()
            .get_by_name(seed_org_id(), "no-such-repo")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn test_repo_unique_per_org() {
    let (db, _pg) = setup().await;

    db.commit_repositories()
        .insert(
            "unique-test".to_string(),
            seed_org_id(),
            seed_api_key_id(),
            None,
        )
        .await
        .unwrap();

    let result = db
        .commit_repositories()
        .insert(
            "unique-test".to_string(),
            seed_org_id(),
            seed_api_key_id(),
            None,
        )
        .await;

    assert!(result.is_err(), "expected unique constraint violation");
    let err = result.unwrap_err();
    let db_err = err.as_db_error().expect("should be a DB error");
    assert_eq!(db_err.constraint(), Some("unique_repo_per_org"));
}

#[tokio::test]
async fn test_repo_list_by_org() {
    let (db, _pg) = setup().await;

    assert!(
        db.commit_repositories()
            .list_by_org(seed_org_id())
            .await
            .unwrap()
            .is_empty()
    );

    for name in ["alpha-app", "beta-service", "gamma-lib"] {
        db.commit_repositories()
            .insert(name.to_string(), seed_org_id(), seed_api_key_id(), None)
            .await
            .unwrap();
    }

    let repos = db
        .commit_repositories()
        .list_by_org(seed_org_id())
        .await
        .unwrap();
    assert_eq!(repos.len(), 3);

    let names: Vec<&str> = repos.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, vec!["alpha-app", "beta-service", "gamma-lib"]);

    assert!(
        db.commit_repositories()
            .list_by_org(Uuid::new_v4())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn test_repo_delete() {
    let (db, _pg) = setup().await;

    let repo = db
        .commit_repositories()
        .insert(
            "to-delete".to_string(),
            seed_org_id(),
            seed_api_key_id(),
            None,
        )
        .await
        .unwrap();

    assert!(
        db.commit_repositories()
            .get_by_id(repo.id)
            .await
            .unwrap()
            .is_some()
    );

    assert!(db.commit_repositories().delete(repo.id).await.unwrap());
    assert!(
        db.commit_repositories()
            .get_by_id(repo.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(!db.commit_repositories().delete(repo.id).await.unwrap());
}

#[tokio::test]
async fn test_repo_delete_cascades_tags() {
    let (db, _pg) = setup().await;
    let (_vm_id, commit_id) = create_test_commit(&db, "200").await;

    let repo = db
        .commit_repositories()
        .insert(
            "cascade-repo".to_string(),
            seed_org_id(),
            seed_api_key_id(),
            None,
        )
        .await
        .unwrap();

    let tag = db
        .commit_tags()
        .insert_with_repo(
            "v1.0".to_string(),
            commit_id,
            seed_api_key_id(),
            seed_org_id(),
            repo.id,
            None,
        )
        .await
        .unwrap();

    assert!(db.commit_tags().get_by_id(tag.id).await.unwrap().is_some());

    db.commit_repositories().delete(repo.id).await.unwrap();

    assert!(db.commit_tags().get_by_id(tag.id).await.unwrap().is_none());
}

// ── Repo-Scoped Tags ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_repo_tag_insert_and_get() {
    let (db, _pg) = setup().await;
    let (_vm_id, commit_id) = create_test_commit(&db, "201").await;

    let repo = db
        .commit_repositories()
        .insert(
            "tag-test-repo".to_string(),
            seed_org_id(),
            seed_api_key_id(),
            None,
        )
        .await
        .unwrap();

    let tag = db
        .commit_tags()
        .insert_with_repo(
            "latest".to_string(),
            commit_id,
            seed_api_key_id(),
            seed_org_id(),
            repo.id,
            Some("The latest build".to_string()),
        )
        .await
        .unwrap();

    assert_eq!(tag.tag_name, "latest");
    assert_eq!(tag.commit_id, commit_id);
    assert_eq!(tag.repo_id, Some(repo.id));
    assert_eq!(tag.description, Some("The latest build".to_string()));

    let fetched = db
        .commit_tags()
        .get_by_repo_and_name(repo.id, "latest")
        .await
        .unwrap()
        .expect("tag should exist");
    assert_eq!(fetched.id, tag.id);
    assert_eq!(fetched.repo_id, Some(repo.id));
}

#[tokio::test]
async fn test_repo_tag_unique_per_repo() {
    let (db, _pg) = setup().await;
    let (_vm_id, commit_id) = create_test_commit(&db, "202").await;

    let repo = db
        .commit_repositories()
        .insert(
            "unique-tag-repo".to_string(),
            seed_org_id(),
            seed_api_key_id(),
            None,
        )
        .await
        .unwrap();

    db.commit_tags()
        .insert_with_repo(
            "v1.0".to_string(),
            commit_id,
            seed_api_key_id(),
            seed_org_id(),
            repo.id,
            None,
        )
        .await
        .unwrap();

    let result = db
        .commit_tags()
        .insert_with_repo(
            "v1.0".to_string(),
            commit_id,
            seed_api_key_id(),
            seed_org_id(),
            repo.id,
            None,
        )
        .await;

    assert!(result.is_err(), "expected unique constraint violation");
    let err = result.unwrap_err();
    let db_err = err.as_db_error().expect("should be a DB error");
    assert_eq!(db_err.constraint(), Some("unique_tag_per_repo"));
}

#[tokio::test]
async fn test_repo_tag_same_name_different_repos() {
    let (db, _pg) = setup().await;
    let (_vm_id, commit_id) = create_test_commit(&db, "203").await;

    let repo_a = db
        .commit_repositories()
        .insert("app-a".to_string(), seed_org_id(), seed_api_key_id(), None)
        .await
        .unwrap();
    let repo_b = db
        .commit_repositories()
        .insert("app-b".to_string(), seed_org_id(), seed_api_key_id(), None)
        .await
        .unwrap();

    let tag_a = db
        .commit_tags()
        .insert_with_repo(
            "latest".to_string(),
            commit_id,
            seed_api_key_id(),
            seed_org_id(),
            repo_a.id,
            None,
        )
        .await
        .unwrap();
    let tag_b = db
        .commit_tags()
        .insert_with_repo(
            "latest".to_string(),
            commit_id,
            seed_api_key_id(),
            seed_org_id(),
            repo_b.id,
            None,
        )
        .await
        .unwrap();

    assert_ne!(tag_a.id, tag_b.id);
    assert_eq!(tag_a.tag_name, "latest");
    assert_eq!(tag_b.tag_name, "latest");
    assert_eq!(tag_a.repo_id, Some(repo_a.id));
    assert_eq!(tag_b.repo_id, Some(repo_b.id));
}

#[tokio::test]
async fn test_repo_tag_list_by_repo() {
    let (db, _pg) = setup().await;
    let (_vm_id, commit_id) = create_test_commit(&db, "204").await;

    let repo = db
        .commit_repositories()
        .insert(
            "list-tags-repo".to_string(),
            seed_org_id(),
            seed_api_key_id(),
            None,
        )
        .await
        .unwrap();

    for name in ["latest", "stable", "v1.0"] {
        db.commit_tags()
            .insert_with_repo(
                name.to_string(),
                commit_id,
                seed_api_key_id(),
                seed_org_id(),
                repo.id,
                None,
            )
            .await
            .unwrap();
    }

    let tags = db.commit_tags().list_by_repo(repo.id).await.unwrap();
    assert_eq!(tags.len(), 3);

    let names: Vec<&str> = tags.iter().map(|t| t.tag_name.as_str()).collect();
    assert_eq!(names, vec!["latest", "stable", "v1.0"]);
}

#[tokio::test]
async fn test_repo_tag_resolve_ref() {
    let (db, _pg) = setup().await;
    let (_vm1, commit1) = create_test_commit(&db, "205").await;
    let (_vm2, commit2) = create_test_commit(&db, "206").await;

    let repo = db
        .commit_repositories()
        .insert(
            "resolve-repo".to_string(),
            seed_org_id(),
            seed_api_key_id(),
            None,
        )
        .await
        .unwrap();

    db.commit_tags()
        .insert_with_repo(
            "v1".to_string(),
            commit1,
            seed_api_key_id(),
            seed_org_id(),
            repo.id,
            None,
        )
        .await
        .unwrap();
    db.commit_tags()
        .insert_with_repo(
            "v2".to_string(),
            commit2,
            seed_api_key_id(),
            seed_org_id(),
            repo.id,
            None,
        )
        .await
        .unwrap();

    let resolved = db
        .commit_tags()
        .resolve_ref(seed_org_id(), "resolve-repo", "v1")
        .await
        .unwrap()
        .expect("should resolve");
    assert_eq!(resolved.commit_id, commit1);
    assert_eq!(resolved.tag_name, "v1");

    let resolved2 = db
        .commit_tags()
        .resolve_ref(seed_org_id(), "resolve-repo", "v2")
        .await
        .unwrap()
        .expect("should resolve");
    assert_eq!(resolved2.commit_id, commit2);

    // Non-existent repo
    assert!(
        db.commit_tags()
            .resolve_ref(seed_org_id(), "no-such-repo", "v1")
            .await
            .unwrap()
            .is_none()
    );
    // Non-existent tag
    assert!(
        db.commit_tags()
            .resolve_ref(seed_org_id(), "resolve-repo", "v999")
            .await
            .unwrap()
            .is_none()
    );
    // Wrong org
    assert!(
        db.commit_tags()
            .resolve_ref(Uuid::new_v4(), "resolve-repo", "v1")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn test_repo_tag_resolve_ref_after_move() {
    let (db, _pg) = setup().await;
    let (_vm1, commit1) = create_test_commit(&db, "207").await;
    let (_vm2, commit2) = create_test_commit(&db, "208").await;

    let repo = db
        .commit_repositories()
        .insert(
            "move-repo".to_string(),
            seed_org_id(),
            seed_api_key_id(),
            None,
        )
        .await
        .unwrap();

    let tag = db
        .commit_tags()
        .insert_with_repo(
            "latest".to_string(),
            commit1,
            seed_api_key_id(),
            seed_org_id(),
            repo.id,
            None,
        )
        .await
        .unwrap();

    let resolved = db
        .commit_tags()
        .resolve_ref(seed_org_id(), "move-repo", "latest")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resolved.commit_id, commit1);

    // Move the floating pointer
    db.commit_tags()
        .update_commit(tag.id, commit2)
        .await
        .unwrap();

    let resolved2 = db
        .commit_tags()
        .resolve_ref(seed_org_id(), "move-repo", "latest")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resolved2.commit_id, commit2);
}

#[tokio::test]
async fn test_repo_tag_update_via_repo_and_name() {
    let (db, _pg) = setup().await;
    let (_vm1, commit1) = create_test_commit(&db, "209").await;
    let (_vm2, commit2) = create_test_commit(&db, "210").await;

    let repo = db
        .commit_repositories()
        .insert(
            "update-test-repo".to_string(),
            seed_org_id(),
            seed_api_key_id(),
            None,
        )
        .await
        .unwrap();

    let tag = db
        .commit_tags()
        .insert_with_repo(
            "stable".to_string(),
            commit1,
            seed_api_key_id(),
            seed_org_id(),
            repo.id,
            Some("initial description".to_string()),
        )
        .await
        .unwrap();

    let updated = db
        .commit_tags()
        .update(
            tag.id,
            Some(commit2),
            Some(Some("updated description".to_string())),
        )
        .await
        .unwrap();
    assert_eq!(updated.commit_id, commit2);
    assert_eq!(updated.description, Some("updated description".to_string()));
    assert!(updated.updated_at > tag.updated_at);

    let resolved = db
        .commit_tags()
        .resolve_ref(seed_org_id(), "update-test-repo", "stable")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resolved.commit_id, commit2);
}

#[tokio::test]
async fn test_repo_tag_multiple_repos_independent() {
    let (db, _pg) = setup().await;
    let (_vm1, commit1) = create_test_commit(&db, "211").await;
    let (_vm2, commit2) = create_test_commit(&db, "212").await;

    let repo_fe = db
        .commit_repositories()
        .insert(
            "frontend".to_string(),
            seed_org_id(),
            seed_api_key_id(),
            None,
        )
        .await
        .unwrap();
    let repo_be = db
        .commit_repositories()
        .insert(
            "backend".to_string(),
            seed_org_id(),
            seed_api_key_id(),
            None,
        )
        .await
        .unwrap();

    // frontend:latest → commit1, backend:latest → commit2
    db.commit_tags()
        .insert_with_repo(
            "latest".to_string(),
            commit1,
            seed_api_key_id(),
            seed_org_id(),
            repo_fe.id,
            None,
        )
        .await
        .unwrap();
    db.commit_tags()
        .insert_with_repo(
            "latest".to_string(),
            commit2,
            seed_api_key_id(),
            seed_org_id(),
            repo_be.id,
            None,
        )
        .await
        .unwrap();

    let fe = db
        .commit_tags()
        .resolve_ref(seed_org_id(), "frontend", "latest")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fe.commit_id, commit1);

    let be = db
        .commit_tags()
        .resolve_ref(seed_org_id(), "backend", "latest")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(be.commit_id, commit2);

    // Move frontend:latest to commit2
    let fe_tag = db
        .commit_tags()
        .get_by_repo_and_name(repo_fe.id, "latest")
        .await
        .unwrap()
        .unwrap();
    db.commit_tags()
        .update_commit(fe_tag.id, commit2)
        .await
        .unwrap();

    let fe_after = db
        .commit_tags()
        .resolve_ref(seed_org_id(), "frontend", "latest")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fe_after.commit_id, commit2);

    // backend unchanged
    let be_after = db
        .commit_tags()
        .resolve_ref(seed_org_id(), "backend", "latest")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(be_after.commit_id, commit2);
}

// ── Public Repositories ─────────────────────────────────────────────────

#[tokio::test]
async fn test_repo_set_public() {
    let (db, _pg) = setup().await;

    let repo = db
        .commit_repositories()
        .insert(
            "pub-test".to_string(),
            seed_org_id(),
            seed_api_key_id(),
            None,
        )
        .await
        .unwrap();

    assert!(!repo.is_public);

    assert!(
        db.commit_repositories()
            .set_public(repo.id, true)
            .await
            .unwrap()
    );

    let fetched = db
        .commit_repositories()
        .get_by_id(repo.id)
        .await
        .unwrap()
        .unwrap();
    assert!(fetched.is_public);

    assert!(
        db.commit_repositories()
            .set_public(repo.id, false)
            .await
            .unwrap()
    );

    let fetched2 = db
        .commit_repositories()
        .get_by_id(repo.id)
        .await
        .unwrap()
        .unwrap();
    assert!(!fetched2.is_public);
}

#[tokio::test]
async fn test_repo_list_public() {
    let (db, _pg) = setup().await;

    // No public repos initially
    assert!(
        db.commit_repositories()
            .list_public()
            .await
            .unwrap()
            .is_empty()
    );

    let _private = db
        .commit_repositories()
        .insert(
            "private-repo".to_string(),
            seed_org_id(),
            seed_api_key_id(),
            None,
        )
        .await
        .unwrap();
    let public1 = db
        .commit_repositories()
        .insert(
            "public-alpha".to_string(),
            seed_org_id(),
            seed_api_key_id(),
            None,
        )
        .await
        .unwrap();
    let public2 = db
        .commit_repositories()
        .insert(
            "public-beta".to_string(),
            seed_org_id(),
            seed_api_key_id(),
            None,
        )
        .await
        .unwrap();

    db.commit_repositories()
        .set_public(public1.id, true)
        .await
        .unwrap();
    db.commit_repositories()
        .set_public(public2.id, true)
        .await
        .unwrap();

    let public_repos = db.commit_repositories().list_public().await.unwrap();
    assert_eq!(public_repos.len(), 2);

    let names: Vec<&str> = public_repos.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"public-alpha"));
    assert!(names.contains(&"public-beta"));
    assert!(!names.contains(&"private-repo"));

    // Private repo should still be in org listing
    let all_repos = db
        .commit_repositories()
        .list_by_org(seed_org_id())
        .await
        .unwrap();
    assert_eq!(all_repos.len(), 3);
}

#[tokio::test]
async fn test_repo_get_public_by_org_and_name() {
    let (db, _pg) = setup().await;

    let repo = db
        .commit_repositories()
        .insert("findme".to_string(), seed_org_id(), seed_api_key_id(), None)
        .await
        .unwrap();

    // Not public yet — should not be found
    assert!(
        db.commit_repositories()
            .get_public_by_org_and_name("test_user", "findme")
            .await
            .unwrap()
            .is_none()
    );

    db.commit_repositories()
        .set_public(repo.id, true)
        .await
        .unwrap();

    // Now it should be found
    let found = db
        .commit_repositories()
        .get_public_by_org_and_name("test_user", "findme")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.id, repo.id);
    assert!(found.is_public);

    // Wrong org name
    assert!(
        db.commit_repositories()
            .get_public_by_org_and_name("nonexistent_org", "findme")
            .await
            .unwrap()
            .is_none()
    );
    // Wrong repo name
    assert!(
        db.commit_repositories()
            .get_public_by_org_and_name("test_user", "nope")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn test_resolve_public_ref() {
    let (db, _pg) = setup().await;
    let (_vm_id, commit_id) = create_test_commit(&db, "220").await;

    let repo = db
        .commit_repositories()
        .insert(
            "pub-resolve".to_string(),
            seed_org_id(),
            seed_api_key_id(),
            None,
        )
        .await
        .unwrap();
    db.commit_tags()
        .insert_with_repo(
            "v1.0".to_string(),
            commit_id,
            seed_api_key_id(),
            seed_org_id(),
            repo.id,
            None,
        )
        .await
        .unwrap();

    // Not public yet — should not resolve
    assert!(
        db.commit_tags()
            .resolve_public_ref("test_user", "pub-resolve", "v1.0")
            .await
            .unwrap()
            .is_none()
    );

    // Make public
    db.commit_repositories()
        .set_public(repo.id, true)
        .await
        .unwrap();

    // Now it should resolve
    let resolved = db
        .commit_tags()
        .resolve_public_ref("test_user", "pub-resolve", "v1.0")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resolved.commit_id, commit_id);

    // Wrong org/repo/tag
    assert!(
        db.commit_tags()
            .resolve_public_ref("wrong_org", "pub-resolve", "v1.0")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        db.commit_tags()
            .resolve_public_ref("test_user", "wrong-repo", "v1.0")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        db.commit_tags()
            .resolve_public_ref("test_user", "pub-resolve", "wrong-tag")
            .await
            .unwrap()
            .is_none()
    );
}

// ── Visibility Revocation ───────────────────────────────────────────────

#[tokio::test]
async fn test_resolve_public_ref_revoked_after_going_private() {
    let (db, _pg) = setup().await;
    let (_vm_id, commit_id) = create_test_commit(&db, "230").await;

    let repo = db
        .commit_repositories()
        .insert(
            "revoke-test".to_string(),
            seed_org_id(),
            seed_api_key_id(),
            None,
        )
        .await
        .unwrap();
    db.commit_tags()
        .insert_with_repo(
            "latest".to_string(),
            commit_id,
            seed_api_key_id(),
            seed_org_id(),
            repo.id,
            None,
        )
        .await
        .unwrap();

    // Make public, verify it resolves
    db.commit_repositories()
        .set_public(repo.id, true)
        .await
        .unwrap();
    assert!(
        db.commit_tags()
            .resolve_public_ref("test_user", "revoke-test", "latest")
            .await
            .unwrap()
            .is_some()
    );

    // Revoke public access
    db.commit_repositories()
        .set_public(repo.id, false)
        .await
        .unwrap();

    // Should no longer resolve via public ref
    assert!(
        db.commit_tags()
            .resolve_public_ref("test_user", "revoke-test", "latest")
            .await
            .unwrap()
            .is_none()
    );

    // Should also disappear from public listings
    let public = db.commit_repositories().list_public().await.unwrap();
    assert!(public.iter().all(|r| r.name != "revoke-test"));

    // But should still resolve via private (org-scoped) ref
    let private = db
        .commit_tags()
        .resolve_ref(seed_org_id(), "revoke-test", "latest")
        .await
        .unwrap();
    assert!(private.is_some());
    assert_eq!(private.unwrap().commit_id, commit_id);
}

#[tokio::test]
async fn test_public_ref_after_tag_moved() {
    let (db, _pg) = setup().await;
    let (_vm1, commit1) = create_test_commit(&db, "231").await;
    let (_vm2, commit2) = create_test_commit(&db, "232").await;

    let repo = db
        .commit_repositories()
        .insert(
            "move-pub".to_string(),
            seed_org_id(),
            seed_api_key_id(),
            None,
        )
        .await
        .unwrap();
    db.commit_repositories()
        .set_public(repo.id, true)
        .await
        .unwrap();

    let tag = db
        .commit_tags()
        .insert_with_repo(
            "latest".to_string(),
            commit1,
            seed_api_key_id(),
            seed_org_id(),
            repo.id,
            None,
        )
        .await
        .unwrap();

    // Resolves to commit1
    let resolved = db
        .commit_tags()
        .resolve_public_ref("test_user", "move-pub", "latest")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resolved.commit_id, commit1);

    // Move tag to commit2
    db.commit_tags()
        .update_commit(tag.id, commit2)
        .await
        .unwrap();

    // Now resolves to commit2
    let resolved2 = db
        .commit_tags()
        .resolve_public_ref("test_user", "move-pub", "latest")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resolved2.commit_id, commit2);
}

#[tokio::test]
async fn test_delete_public_repo_removes_from_public_listing() {
    let (db, _pg) = setup().await;

    let repo = db
        .commit_repositories()
        .insert(
            "delete-pub".to_string(),
            seed_org_id(),
            seed_api_key_id(),
            None,
        )
        .await
        .unwrap();
    db.commit_repositories()
        .set_public(repo.id, true)
        .await
        .unwrap();

    assert_eq!(
        db.commit_repositories().list_public().await.unwrap().len(),
        1
    );

    db.commit_repositories().delete(repo.id).await.unwrap();

    assert!(
        db.commit_repositories()
            .list_public()
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        db.commit_repositories()
            .get_public_by_org_and_name("test_user", "delete-pub")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn test_set_public_nonexistent_repo() {
    let (db, _pg) = setup().await;

    // set_public on a nonexistent repo returns false (0 rows updated)
    let result = db
        .commit_repositories()
        .set_public(Uuid::new_v4(), true)
        .await
        .unwrap();
    assert!(!result);
}

#[tokio::test]
async fn test_public_repo_tags_cascade_on_delete() {
    let (db, _pg) = setup().await;
    let (_vm_id, commit_id) = create_test_commit(&db, "233").await;

    let repo = db
        .commit_repositories()
        .insert(
            "cascade-pub".to_string(),
            seed_org_id(),
            seed_api_key_id(),
            None,
        )
        .await
        .unwrap();
    db.commit_repositories()
        .set_public(repo.id, true)
        .await
        .unwrap();

    let tag = db
        .commit_tags()
        .insert_with_repo(
            "v1".to_string(),
            commit_id,
            seed_api_key_id(),
            seed_org_id(),
            repo.id,
            None,
        )
        .await
        .unwrap();

    // Tag resolves publicly
    assert!(
        db.commit_tags()
            .resolve_public_ref("test_user", "cascade-pub", "v1")
            .await
            .unwrap()
            .is_some()
    );

    // Delete repo — tags should cascade
    db.commit_repositories().delete(repo.id).await.unwrap();

    assert!(db.commit_tags().get_by_id(tag.id).await.unwrap().is_none());
    assert!(
        db.commit_tags()
            .resolve_public_ref("test_user", "cascade-pub", "v1")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn test_get_public_by_org_and_name_ignores_private_repos() {
    let (db, _pg) = setup().await;

    // Create a private repo
    db.commit_repositories()
        .insert(
            "private-only".to_string(),
            seed_org_id(),
            seed_api_key_id(),
            None,
        )
        .await
        .unwrap();

    // Should NOT be findable via public query
    assert!(
        db.commit_repositories()
            .get_public_by_org_and_name("test_user", "private-only")
            .await
            .unwrap()
            .is_none()
    );

    // But should be findable via org-scoped query
    assert!(
        db.commit_repositories()
            .get_by_name(seed_org_id(), "private-only")
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn test_resolve_public_ref_private_repo_with_same_name() {
    let (db, _pg) = setup().await;
    let (_vm_id, commit_id) = create_test_commit(&db, "234").await;

    // Private repo with a tag
    let repo = db
        .commit_repositories()
        .insert(
            "shadowed".to_string(),
            seed_org_id(),
            seed_api_key_id(),
            None,
        )
        .await
        .unwrap();
    db.commit_tags()
        .insert_with_repo(
            "v1".to_string(),
            commit_id,
            seed_api_key_id(),
            seed_org_id(),
            repo.id,
            None,
        )
        .await
        .unwrap();

    // Should NOT resolve publicly even though it exists privately
    assert!(
        db.commit_tags()
            .resolve_public_ref("test_user", "shadowed", "v1")
            .await
            .unwrap()
            .is_none()
    );

    // Should resolve via org-scoped ref
    assert!(
        db.commit_tags()
            .resolve_ref(seed_org_id(), "shadowed", "v1")
            .await
            .unwrap()
            .is_some()
    );
}

// ── Fork Independence ───────────────────────────────────────────────────

/// Simulates the DB-level effects of a fork and verifies the fork remains
/// fully functional after the source repo goes private.
///
/// The real fork action (which needs Ceph/nodes) does:
///   1. resolve_public_ref → source commit
///   2. branch VM (Ceph CoW clone)
///   3. commit the new VM → forker's own commit
///   4. create repo + tag in forker's org pointing to forker's commit
///
/// This test simulates steps 3-4 at the DB level and verifies independence.
#[tokio::test]
async fn test_fork_independent_after_source_goes_private() {
    let (db, _pg) = setup().await;

    // -- Source org sets up a public repo with a tagged commit --
    let (_src_vm, src_commit) = create_test_commit(&db, "240").await;
    let source_repo = db
        .commit_repositories()
        .insert(
            "base-image".to_string(),
            seed_org_id(),
            seed_api_key_id(),
            None,
        )
        .await
        .unwrap();
    db.commit_repositories()
        .set_public(source_repo.id, true)
        .await
        .unwrap();
    db.commit_tags()
        .insert_with_repo(
            "latest".to_string(),
            src_commit,
            seed_api_key_id(),
            seed_org_id(),
            source_repo.id,
            None,
        )
        .await
        .unwrap();

    // Verify source is publicly resolvable
    let resolved = db
        .commit_tags()
        .resolve_public_ref("test_user", "base-image", "latest")
        .await
        .unwrap();
    assert!(resolved.is_some());
    assert_eq!(resolved.unwrap().commit_id, src_commit);

    // -- Simulate the fork: forker creates their own commit + repo + tag --
    // In reality this commit comes from committing a branched VM, but at the
    // DB level it's just a new commit row owned by the forker.
    let forker_vm = Uuid::new_v4();
    let forker_commit = Uuid::new_v4();
    let forker_ip: std::net::Ipv6Addr = "fd00:fe11:deed:1::f241".parse().unwrap();

    db.vms()
        .insert(
            forker_vm,
            Some(src_commit),
            None,
            seed_node_id(),
            forker_ip,
            "fork-priv".to_string(),
            "fork-pub".to_string(),
            53240,
            seed_api_key_id(),
            chrono::Utc::now(),
            None,
            4,
            512,
        )
        .await
        .unwrap();

    db.commits()
        .insert(
            forker_commit,
            Some(forker_vm),
            Some(src_commit),
            seed_api_key_id(),
            "fork: test_user/base-image:latest".to_string(),
            None,
            chrono::Utc::now(),
            false,
        )
        .await
        .unwrap();

    let fork_repo = db
        .commit_repositories()
        .insert(
            "base-image".to_string(),
            seed_org_id(),
            seed_api_key_id(),
            Some("Forked from test_user/base-image".to_string()),
        )
        .await;

    // Same org same name would conflict — use a different name as the fork would
    let fork_repo = match fork_repo {
        Ok(r) => r,
        Err(_) => {
            // In practice the forker is in a DIFFERENT org so the name wouldn't conflict.
            // For this single-org test, use a different name.
            db.commit_repositories()
                .insert(
                    "my-base-image".to_string(),
                    seed_org_id(),
                    seed_api_key_id(),
                    Some("Forked from test_user/base-image".to_string()),
                )
                .await
                .unwrap()
        }
    };

    db.commit_tags()
        .insert_with_repo(
            "latest".to_string(),
            forker_commit,
            seed_api_key_id(),
            seed_org_id(),
            fork_repo.id,
            None,
        )
        .await
        .unwrap();

    // Verify fork resolves to forker's own commit (not source)
    let fork_resolved = db
        .commit_tags()
        .resolve_ref(seed_org_id(), &fork_repo.name, "latest")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fork_resolved.commit_id, forker_commit);
    assert_ne!(fork_resolved.commit_id, src_commit);

    // -- Source goes private --
    db.commit_repositories()
        .set_public(source_repo.id, false)
        .await
        .unwrap();

    // Source is no longer publicly resolvable
    assert!(
        db.commit_tags()
            .resolve_public_ref("test_user", "base-image", "latest")
            .await
            .unwrap()
            .is_none()
    );

    // Fork is STILL fully functional — it has its own commit
    let fork_still_works = db
        .commit_tags()
        .resolve_ref(seed_org_id(), &fork_repo.name, "latest")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fork_still_works.commit_id, forker_commit);

    // Fork's commit is independently accessible
    let commit = db.commits().get_by_id(forker_commit).await.unwrap();
    assert!(commit.is_some());

    // -- Even deleting the source repo doesn't affect the fork --
    db.commit_repositories()
        .delete(source_repo.id)
        .await
        .unwrap();

    let fork_survives_delete = db
        .commit_tags()
        .resolve_ref(seed_org_id(), &fork_repo.name, "latest")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fork_survives_delete.commit_id, forker_commit);

    // Forker's commit still exists (it's not in the source repo)
    assert!(
        db.commits()
            .get_by_id(forker_commit)
            .await
            .unwrap()
            .is_some()
    );
}

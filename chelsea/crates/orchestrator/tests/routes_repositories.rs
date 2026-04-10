use axum::http::{Method, StatusCode};
use orch_test::{
    ActionTestEnv,
    client::TestClient,
    db::{ChelseaNodeRepository, NodeResources, VMCommitsRepository, VMsRepository},
};
use uuid::Uuid;

const SEED_API_KEY_ID: &str = "ef90fd52-66b5-47e7-b7dc-e73c4381028f";
const SEED_NODE_ID: &str = "4569f1fe-054b-4e8d-855a-f3545167f8a9";

/// Ensure the seed node exists (idempotent — safe to call multiple times).
async fn ensure_seed_node(env: &ActionTestEnv) -> Uuid {
    let node_id: Uuid = SEED_NODE_ID.parse().unwrap();
    if env.db().node().get_by_id(&node_id).await.unwrap().is_none() {
        env.db()
            .node()
            .insert(
                node_id,
                env.orch.id(),
                &NodeResources::new(96, 193025, 1000000, 64),
                "seed-node-priv-key",
                "seed-node-pub-key",
                Some("fd00:fe11:deed:0::100".parse().unwrap()),
                Some("10.0.0.1".parse().unwrap()),
            )
            .await
            .unwrap();
    }
    node_id
}

/// Helper: create a VM + commit in the DB so we have a valid commit_id for tagging.
async fn create_test_commit(env: &ActionTestEnv, suffix: &str) -> (Uuid, Uuid) {
    let vm_id = Uuid::new_v4();
    let commit_id = Uuid::new_v4();
    let ip: std::net::Ipv6Addr = format!("fd00:fe11:deed:1::e{suffix}")
        .parse()
        .unwrap_or_else(|_| format!("fd00:fe11:deed:1::ee{suffix}").parse().unwrap());

    let seed_api_key_id: Uuid = SEED_API_KEY_ID.parse().unwrap();
    let seed_node_id = ensure_seed_node(env).await;

    env.db()
        .vms()
        .insert(
            vm_id,
            None,
            None,
            seed_node_id,
            ip,
            format!("priv-{suffix}"),
            format!("pub-{suffix}"),
            54000 + suffix.parse::<u16>().unwrap_or(0),
            seed_api_key_id,
            chrono::Utc::now(),
            None,
            4,
            512,
        )
        .await
        .unwrap();

    env.db()
        .commits()
        .insert(
            commit_id,
            Some(vm_id),
            None,
            seed_api_key_id,
            format!("test-commit-{suffix}"),
            None,
            chrono::Utc::now(),
            false,
        )
        .await
        .unwrap();

    (vm_id, commit_id)
}

// ── Repository CRUD Routes ──────────────────────────────────────────────

#[test]
fn route_create_repository_without_api_key() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let client = TestClient::new(env.inbound());
        let res = client.create_repository("test-repo", None).await;
        assert_eq!(res.unwrap_err(), orch_test::client::TestError::Unauthorized);
    })
}

#[test]
fn route_repository_crud() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());

        // Create
        let repo = client
            .create_repository("my-app", Some("My application"))
            .await
            .expect("create repo failed");
        assert_eq!(repo.name, "my-app");

        // Get
        let fetched = client
            .get_repository("my-app")
            .await
            .expect("get repo failed");
        assert_eq!(fetched.repo_id, repo.repo_id);
        assert_eq!(fetched.name, "my-app");
        assert_eq!(fetched.description, Some("My application".to_string()));
        assert!(!fetched.is_public);

        // List
        let list = client.list_repositories().await.expect("list repos failed");
        assert!(list.repositories.iter().any(|r| r.name == "my-app"));

        // Delete
        client
            .delete_repository("my-app")
            .await
            .expect("delete repo failed");

        // Verify deleted
        let (status, _) = client
            .raw_request(Method::GET, "/api/v1/repositories/my-app", None)
            .await
            .unwrap();
        assert_eq!(status, StatusCode::NOT_FOUND);
    })
}

#[test]
fn route_create_duplicate_repository() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());

        client.create_repository("dup-repo", None).await.unwrap();

        let (status, _) = client
            .raw_request(
                Method::POST,
                "/api/v1/repositories",
                Some(r#"{"name": "dup-repo"}"#.to_string()),
            )
            .await
            .unwrap();
        assert_eq!(status, StatusCode::CONFLICT);
    })
}

#[test]
fn route_get_nonexistent_repository() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());

        let (status, _) = client
            .raw_request(Method::GET, "/api/v1/repositories/no-such-repo", None)
            .await
            .unwrap();
        assert_eq!(status, StatusCode::NOT_FOUND);
    })
}

// ── Repository Tag Routes ───────────────────────────────────────────────

#[test]
fn route_repo_tag_crud() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());
        let (_vm_id, commit_id) = create_test_commit(env, "300").await;

        // Create repo
        client.create_repository("tag-test", None).await.unwrap();

        // Create tag
        let tag = client
            .create_repo_tag("tag-test", "v1.0", commit_id, Some("First release"))
            .await
            .expect("create tag failed");
        assert_eq!(tag.reference, "tag-test:v1.0");
        assert_eq!(tag.commit_id, commit_id);

        // Get tag
        let fetched = client
            .get_repo_tag("tag-test", "v1.0")
            .await
            .expect("get tag failed");
        assert_eq!(fetched.tag_id, tag.tag_id);
        assert_eq!(fetched.tag_name, "v1.0");
        assert_eq!(fetched.commit_id, commit_id);
        assert_eq!(fetched.description, Some("First release".to_string()));

        // List tags
        let tags = client
            .list_repo_tags("tag-test")
            .await
            .expect("list tags failed");
        assert_eq!(tags.tags.len(), 1);
        assert_eq!(tags.repository, "tag-test");

        // Delete tag
        client
            .delete_repo_tag("tag-test", "v1.0")
            .await
            .expect("delete tag failed");

        // Verify deleted
        let (status, _) = client
            .raw_request(Method::GET, "/api/v1/repositories/tag-test/tags/v1.0", None)
            .await
            .unwrap();
        assert_eq!(status, StatusCode::NOT_FOUND);
    })
}

#[test]
fn route_create_tag_nonexistent_repo() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());

        let (status, _) = client
            .raw_request(
                Method::POST,
                "/api/v1/repositories/no-such-repo/tags",
                Some(format!(
                    r#"{{"tag_name": "v1", "commit_id": "{}"}}"#,
                    Uuid::new_v4()
                )),
            )
            .await
            .unwrap();
        assert_eq!(status, StatusCode::NOT_FOUND);
    })
}

#[test]
fn route_create_duplicate_tag() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());
        let (_vm_id, commit_id) = create_test_commit(env, "301").await;

        client
            .create_repository("dup-tag-test", None)
            .await
            .unwrap();
        client
            .create_repo_tag("dup-tag-test", "latest", commit_id, None)
            .await
            .unwrap();

        let (status, _) = client
            .raw_request(
                Method::POST,
                "/api/v1/repositories/dup-tag-test/tags",
                Some(format!(
                    r#"{{"tag_name": "latest", "commit_id": "{}"}}"#,
                    commit_id
                )),
            )
            .await
            .unwrap();
        assert_eq!(status, StatusCode::CONFLICT);
    })
}

// ── Visibility Routes ───────────────────────────────────────────────────

#[test]
fn route_set_visibility() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());

        client.create_repository("vis-test", None).await.unwrap();

        // Initially private
        let repo = client.get_repository("vis-test").await.unwrap();
        assert!(!repo.is_public);

        // Make public
        client
            .set_repository_visibility("vis-test", true)
            .await
            .expect("set visibility failed");

        let repo = client.get_repository("vis-test").await.unwrap();
        assert!(repo.is_public);

        // Make private again
        client
            .set_repository_visibility("vis-test", false)
            .await
            .unwrap();

        let repo = client.get_repository("vis-test").await.unwrap();
        assert!(!repo.is_public);
    })
}

#[test]
fn route_set_visibility_without_auth() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let client = TestClient::new(env.inbound());

        let (status, _) = client
            .raw_request(
                Method::PATCH,
                "/api/v1/repositories/anything/visibility",
                Some(r#"{"is_public": true}"#.to_string()),
            )
            .await
            .unwrap();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    })
}

// ── Public Repository Routes (unauthenticated) ─────────────────────────

#[test]
fn route_public_repositories() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let authed = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());
        let anon = TestClient::new(env.inbound()); // no auth

        let (_vm_id, commit_id) = create_test_commit(env, "310").await;

        // Create a repo and make it public
        authed
            .create_repository("pub-browse", Some("A public repo"))
            .await
            .unwrap();
        authed
            .set_repository_visibility("pub-browse", true)
            .await
            .unwrap();
        authed
            .create_repo_tag("pub-browse", "v1.0", commit_id, None)
            .await
            .unwrap();

        // Create a private repo
        authed.create_repository("priv-hidden", None).await.unwrap();

        // Anon: list public repos — should see pub-browse only
        let public = anon
            .list_public_repositories()
            .await
            .expect("list public failed");
        assert!(public.repositories.iter().any(|r| r.name == "pub-browse"));
        assert!(!public.repositories.iter().any(|r| r.name == "priv-hidden"));

        // Anon: get public repo by org/name
        let repo = anon
            .get_public_repository("test_user", "pub-browse")
            .await
            .expect("get public repo failed");
        assert_eq!(repo.name, "pub-browse");
        assert_eq!(repo.org_name, "test_user");
        assert_eq!(repo.full_name, "test_user/pub-browse");

        // Anon: list tags in public repo
        let tags = anon
            .list_public_repo_tags("test_user", "pub-browse")
            .await
            .expect("list public tags failed");
        assert_eq!(tags.tags.len(), 1);
        assert_eq!(tags.tags[0].tag_name, "v1.0");

        // Anon: get specific tag
        let tag = anon
            .get_public_repo_tag("test_user", "pub-browse", "v1.0")
            .await
            .expect("get public tag failed");
        assert_eq!(tag.commit_id, commit_id);

        // Anon: private repo should not be accessible
        let (status, _) = anon
            .raw_request(
                Method::GET,
                "/api/v1/public/repositories/test_user/priv-hidden",
                None,
            )
            .await
            .unwrap();
        assert_eq!(status, StatusCode::NOT_FOUND);

        // Anon: nonexistent repo
        let (status, _) = anon
            .raw_request(
                Method::GET,
                "/api/v1/public/repositories/test_user/no-such-repo",
                None,
            )
            .await
            .unwrap();
        assert_eq!(status, StatusCode::NOT_FOUND);

        // Anon: nonexistent tag in public repo
        let (status, _) = anon
            .raw_request(
                Method::GET,
                "/api/v1/public/repositories/test_user/pub-browse/tags/no-such-tag",
                None,
            )
            .await
            .unwrap();
        assert_eq!(status, StatusCode::NOT_FOUND);
    })
}

#[test]
fn route_public_repo_visibility_revocation() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let authed = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());
        let anon = TestClient::new(env.inbound());

        let (_vm_id, commit_id) = create_test_commit(env, "311").await;

        authed.create_repository("revoke-me", None).await.unwrap();
        authed
            .set_repository_visibility("revoke-me", true)
            .await
            .unwrap();
        authed
            .create_repo_tag("revoke-me", "latest", commit_id, None)
            .await
            .unwrap();

        // Anon can see it
        anon.get_public_repository("test_user", "revoke-me")
            .await
            .expect("should be visible");

        // Revoke public access
        authed
            .set_repository_visibility("revoke-me", false)
            .await
            .unwrap();

        // Anon can no longer see it
        let (status, _) = anon
            .raw_request(
                Method::GET,
                "/api/v1/public/repositories/test_user/revoke-me",
                None,
            )
            .await
            .unwrap();
        assert_eq!(status, StatusCode::NOT_FOUND);

        // But authed user still can via private route
        let repo = authed
            .get_repository("revoke-me")
            .await
            .expect("owner should still see it");
        assert_eq!(repo.name, "revoke-me");
    })
}

// ── Fork Routes ─────────────────────────────────────────────────────────

#[test]
fn route_fork_without_auth() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let client = TestClient::new(env.inbound());

        let (status, _) = client
            .raw_request(
                Method::POST,
                "/api/v1/repositories/fork",
                Some(r#"{"source_org":"x","source_repo":"y","source_tag":"z"}"#.to_string()),
            )
            .await
            .unwrap();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    })
}

#[test]
fn route_fork_nonexistent_source() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());

        let (status, _) = client
            .raw_request(
                Method::POST,
                "/api/v1/repositories/fork",
                Some(
                    r#"{"source_org":"no-org","source_repo":"no-repo","source_tag":"no-tag"}"#
                        .to_string(),
                ),
            )
            .await
            .unwrap();
        assert_eq!(status, StatusCode::NOT_FOUND);
    })
}

#[test]
fn route_fork_private_repo_fails() {
    ActionTestEnv::with_env_no_wg(|env| async move {
        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());
        let (_vm_id, commit_id) = create_test_commit(env, "320").await;

        // Create private repo with a tag
        client
            .create_repository("private-source", None)
            .await
            .unwrap();
        client
            .create_repo_tag("private-source", "latest", commit_id, None)
            .await
            .unwrap();

        // Try to fork — should fail because repo is not public
        let (status, _) = client
            .raw_request(
                Method::POST,
                "/api/v1/repositories/fork",
                Some(
                    r#"{"source_org":"test_user","source_repo":"private-source","source_tag":"latest"}"#
                        .to_string(),
                ),
            )
            .await
            .unwrap();
        assert_eq!(status, StatusCode::NOT_FOUND);
    })
}

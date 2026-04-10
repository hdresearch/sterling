//! Integration tests for commit-related functionality.
//!
//! Tests cover:
//! - The grandparent commit bug fix (committing a VM twice)
//! - The ListCommits action (pagination, empty results, edge cases)
//! - The list_commits route (GET /api/v1/commits)
//!
//! These are DB-only tests — no WireGuard or Chelsea required.
//! Run with: cargo nextest run -p orchestrator --test commits

use std::time::Duration;

use chrono::{DateTime, Utc};
use futures_util::FutureExt;
use orch_test::{ActionTestEnv, client::TestClient};
use orchestrator::{
    action::{self, DeleteCommit, DeleteCommitError, GetCommit, ListCommits, SetCommitPublic},
    db::{ApiKeyEntity, ApiKeysRepository, VMCommitsRepository, VMsRepository},
};
use tokio::time::timeout;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Test harness helpers
// ---------------------------------------------------------------------------

macro_rules! commits_test {
    ($name:ident, $timeout_secs:expr, $body:expr) => {
        #[test]
        fn $name() {
            ActionTestEnv::with_env_no_wg(|env| {
                timeout(Duration::from_secs($timeout_secs), async move {
                    #[allow(clippy::redundant_closure_call)]
                    ($body)(env).await;
                })
                .map(|r| r.expect("Test timed out"))
            });
        }
    };
    ($name:ident, $body:expr) => {
        commits_test!($name, 15, $body);
    };
}

/// Seed data constants (from 20251111063619_seed_db.sql)
const SEED_API_KEY_ID: &str = "ef90fd52-66b5-47e7-b7dc-e73c4381028f";
const SEED_NODE_ID: &str = "4569f1fe-054b-4e8d-855a-f3545167f8a9";
const SEED_USER_ID: &str = "9e92f9ad-3c1e-4e70-b5c4-e60de0d646e9";
const SEED_ORG_ID: &str = "2fbd38fd-aaed-4fae-9f9a-f75ae3ef313d";

fn seed_api_key_id() -> Uuid {
    Uuid::parse_str(SEED_API_KEY_ID).unwrap()
}
fn seed_node_id() -> Uuid {
    Uuid::parse_str(SEED_NODE_ID).unwrap()
}
fn seed_user_id() -> Uuid {
    Uuid::parse_str(SEED_USER_ID).unwrap()
}
fn seed_org_id() -> Uuid {
    Uuid::parse_str(SEED_ORG_ID).unwrap()
}
fn seed_account_id() -> Uuid {
    Uuid::parse_str("47750c3a-d1fa-4f33-8135-f972fadfe3bd").unwrap()
}

async fn get_test_api_key(env: &ActionTestEnv) -> ApiKeyEntity {
    let keys = env
        .db()
        .keys()
        .list_valid()
        .await
        .expect("Should list keys");
    keys.into_iter()
        .next()
        .expect("Test database should have at least one API key")
}

/// Create a second API key under the seed org, for ownership isolation tests.
async fn create_second_api_key(env: &ActionTestEnv) -> ApiKeyEntity {
    env.db()
        .keys()
        .insert(
            seed_user_id(),
            seed_org_id(),
            "second-test-key",
            "PBKDF2",
            100,
            "salt-second",
            "hash-second",
            Utc::now(),
            None,
        )
        .await
        .expect("Failed to create second API key")
}

async fn get_commit_deleted_markers(
    env: &ActionTestEnv,
    commit_id: Uuid,
) -> (Option<DateTime<Utc>>, Option<Uuid>) {
    let client = env.db().raw_obj().await;
    let row = client
        .query_opt(
            "SELECT deleted_at, deleted_by FROM commits WHERE commit_id = $1",
            &[&commit_id],
        )
        .await
        .expect("query should succeed");
    match row {
        Some(row) => (row.get("deleted_at"), row.get("deleted_by")),
        None => (None, None),
    }
}

async fn create_other_org_api_key(env: &ActionTestEnv) -> ApiKeyEntity {
    let org_id = Uuid::new_v4();
    let db = env.db();
    let org_name = format!("org-{}", rand_hex());
    let description = format!("Test org {org_name}");

    db.execute(
        "INSERT INTO organizations (org_id, account_id, name, description, billing_contact_id) VALUES ($1, $2, $3, $4, $5)",
        &[&org_id, &seed_account_id(), &org_name, &description, &seed_user_id()],
    )
    .await
    .expect("Failed to insert test organization");

    let salt = format!("salt-{}", rand_hex());
    let hash = format!("hash-{}", rand_hex());
    db.keys()
        .insert(
            seed_user_id(),
            org_id,
            &format!("other-org-key-{}", rand_hex()),
            "PBKDF2",
            100,
            &salt,
            &hash,
            Utc::now(),
            None,
        )
        .await
        .expect("Failed to create other-org API key")
}

/// Seed data for the second org (from 20260312224501_seed_second_org.sql)
const SECOND_ORG_API_KEY_ID: &str = "a1b2c3d4-e5f6-7890-abcd-ef0123456789";

fn second_org_api_key_id() -> Uuid {
    Uuid::parse_str(SECOND_ORG_API_KEY_ID).unwrap()
}

/// Get the API key entity for the second org (cross-org tests).
async fn get_second_org_api_key(env: &ActionTestEnv) -> ApiKeyEntity {
    env.db()
        .keys()
        .get_by_id(second_org_api_key_id())
        .await
        .expect("Should fetch second org key")
        .expect("Second org API key should exist in seed data")
}

/// Helper: insert a VM into the test DB and return its ID.
async fn insert_test_vm(env: &ActionTestEnv) -> Uuid {
    let vm_id = Uuid::new_v4();
    let ip = format!("fd00:fe11:deed:1::{}", rand_hex());
    env.db()
        .vms()
        .insert(
            vm_id,
            None, // no parent commit
            None, // no grandparent vm
            seed_node_id(),
            ip.parse().unwrap(),
            format!("priv-{vm_id}"),
            format!("pub-{vm_id}"),
            51830 + (vm_id.as_u128() % 10000) as u16,
            seed_api_key_id(),
            Utc::now(),
            None,
            4,
            512,
        )
        .await
        .unwrap_or_else(|e| panic!("Failed to insert test VM: {e}"));
    vm_id
}

/// Helper: insert a VM that was created from a commit (has parent_commit_id).
async fn insert_vm_from_commit(
    env: &ActionTestEnv,
    parent_commit_id: Uuid,
    grandparent_vm_id: Option<Uuid>,
) -> Uuid {
    let vm_id = Uuid::new_v4();
    let ip = format!("fd00:fe11:deed:1::{}", rand_hex());
    env.db()
        .vms()
        .insert(
            vm_id,
            Some(parent_commit_id),
            grandparent_vm_id,
            seed_node_id(),
            ip.parse().unwrap(),
            format!("priv-{vm_id}"),
            format!("pub-{vm_id}"),
            51830 + (vm_id.as_u128() % 10000) as u16,
            seed_api_key_id(),
            Utc::now(),
            None,
            4,
            512,
        )
        .await
        .unwrap_or_else(|e| panic!("Failed to insert test VM from commit: {e}"));
    vm_id
}

/// Simple random hex suffix to avoid IP collisions in tests.
fn rand_hex() -> String {
    format!("{:x}", Uuid::new_v4().as_u128() % 0xFFFF)
}

// ---------------------------------------------------------------------------
// Bug fix: committing a VM twice — grandparent chain
// ---------------------------------------------------------------------------

// The bug: when committing a VM that was created from a commit for the
// second time, the grandparent_commit_id should point to the first commit
// (from get_latest_by_vm), not the original commit the VM was created from.
commits_test!(
    test_commit_vm_twice_grandparent_chain,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let owner_id = seed_api_key_id();

        // 1. Create initial commit (simulates an existing commit in the system)
        let original_commit_id = Uuid::new_v4();
        // We need a VM to be the parent of this commit for FK constraints
        let original_vm_id = insert_test_vm(env).await;

        db.commits()
            .insert(
                original_commit_id,
                Some(original_vm_id),
                None, // root commit, no grandparent
                owner_id,
                "original-commit".to_string(),
                None,
                Utc::now(),
                false,
            )
            .await
            .expect("Failed to insert original commit");

        // 2. Create a VM from that commit (simulates `from_commit`)
        let vm_id = insert_vm_from_commit(env, original_commit_id, Some(original_vm_id)).await;

        // 3. First commit of this VM
        //    The grandparent should be original_commit_id (the commit the VM was created from)
        //    because there's no prior commit from this VM (get_latest_by_vm returns None).
        let first_commit_id = Uuid::new_v4();

        // Simulate the logic from CommitVM::call — check get_latest_by_vm first
        let vm = db.vms().get_by_id(vm_id).await.unwrap().unwrap();
        let grandparent_1 = match db.commits().get_latest_by_vm(vm_id).await.unwrap() {
            Some(latest) => Some(latest.id),
            None => vm.parent_commit_id.clone(),
        };

        assert_eq!(
            grandparent_1,
            Some(original_commit_id),
            "First commit's grandparent should be the commit the VM was created from"
        );

        db.commits()
            .insert(
                first_commit_id,
                Some(vm_id),
                grandparent_1,
                owner_id,
                "first-commit".to_string(),
                None,
                Utc::now(),
                false,
            )
            .await
            .expect("Failed to insert first commit");

        // 4. Second commit of the same VM
        //    The grandparent should now be first_commit_id (from get_latest_by_vm),
        //    NOT original_commit_id. This is the bug that was fixed.
        let second_commit_id = Uuid::new_v4();

        let grandparent_2 = match db.commits().get_latest_by_vm(vm_id).await.unwrap() {
            Some(latest) => Some(latest.id),
            None => vm.parent_commit_id.clone(),
        };

        assert_eq!(
            grandparent_2,
            Some(first_commit_id),
            "Second commit's grandparent should be the first commit, not the original"
        );

        db.commits()
            .insert(
                second_commit_id,
                Some(vm_id),
                grandparent_2,
                owner_id,
                "second-commit".to_string(),
                None,
                Utc::now(),
                false,
            )
            .await
            .expect("Failed to insert second commit");

        // 5. Verify the full chain
        let commit_1 = db
            .commits()
            .get_by_id(first_commit_id)
            .await
            .unwrap()
            .unwrap();
        let commit_2 = db
            .commits()
            .get_by_id(second_commit_id)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(commit_1.grandparent_commit_id, Some(original_commit_id));
        assert_eq!(commit_2.grandparent_commit_id, Some(first_commit_id));
        assert_eq!(commit_1.parent_vm_id, Some(vm_id));
        assert_eq!(commit_2.parent_vm_id, Some(vm_id));
    }
);

// Verify that a root VM (not created from a commit) gets None as grandparent
// on its first commit.
commits_test!(
    test_commit_root_vm_has_no_grandparent,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();

        let vm_id = insert_test_vm(env).await;
        let vm = db.vms().get_by_id(vm_id).await.unwrap().unwrap();

        // Root VM has no parent_commit_id
        assert!(vm.parent_commit_id.is_none());

        let grandparent = match db.commits().get_latest_by_vm(vm_id).await.unwrap() {
            Some(latest) => Some(latest.id),
            None => vm.parent_commit_id.clone(),
        };

        assert_eq!(
            grandparent, None,
            "Root VM's first commit should have no grandparent"
        );
    }
);

// Three sequential commits should form a proper chain.
commits_test!(
    test_commit_chain_three_deep,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let owner_id = seed_api_key_id();

        // Create original commit from a root VM
        let root_vm_id = insert_test_vm(env).await;
        let commit_0 = Uuid::new_v4();
        db.commits()
            .insert(
                commit_0,
                Some(root_vm_id),
                None,
                owner_id,
                "c0".into(),
                None,
                Utc::now(),
                false,
            )
            .await
            .unwrap();

        // VM created from commit_0
        let vm_id = insert_vm_from_commit(env, commit_0, Some(root_vm_id)).await;
        let vm = db.vms().get_by_id(vm_id).await.unwrap().unwrap();

        let mut prev_commit_id: Option<Uuid> = None;
        let commit_ids: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();

        for (i, &cid) in commit_ids.iter().enumerate() {
            let grandparent = match db.commits().get_latest_by_vm(vm_id).await.unwrap() {
                Some(latest) => Some(latest.id),
                None => vm.parent_commit_id.clone(),
            };

            let expected_gp = if i == 0 {
                Some(commit_0)
            } else {
                prev_commit_id
            };
            assert_eq!(grandparent, expected_gp, "Commit {i} grandparent mismatch");

            db.commits()
                .insert(
                    cid,
                    Some(vm_id),
                    grandparent,
                    owner_id,
                    format!("c{}", i + 1),
                    None,
                    Utc::now(),
                    false,
                )
                .await
                .unwrap();

            prev_commit_id = Some(cid);
        }

        // Verify the chain: c1 -> commit_0, c2 -> c1, c3 -> c2
        let c1 = db
            .commits()
            .get_by_id(commit_ids[0])
            .await
            .unwrap()
            .unwrap();
        let c2 = db
            .commits()
            .get_by_id(commit_ids[1])
            .await
            .unwrap()
            .unwrap();
        let c3 = db
            .commits()
            .get_by_id(commit_ids[2])
            .await
            .unwrap()
            .unwrap();

        assert_eq!(c1.grandparent_commit_id, Some(commit_0));
        assert_eq!(c2.grandparent_commit_id, Some(commit_ids[0]));
        assert_eq!(c3.grandparent_commit_id, Some(commit_ids[1]));
    }
);

// ---------------------------------------------------------------------------
// ListCommits action tests
// ---------------------------------------------------------------------------

commits_test!(
    test_list_commits_empty,
    |env: &'static ActionTestEnv| async move {
        let api_key = get_test_api_key(env).await;

        let result = action::call(ListCommits::new(api_key, None, None))
            .await
            .expect("ListCommits should succeed");

        assert_eq!(result.commits.len(), 0);
        assert_eq!(result.total, 0);
        assert_eq!(result.offset, 0);
    }
);

commits_test!(
    test_list_commits_returns_owned_commits,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let api_key = get_test_api_key(env).await;
        let owner_id = api_key.id();

        // Insert a VM and some commits owned by this API key
        let vm_id = insert_test_vm(env).await;
        let mut commit_ids = Vec::new();
        for i in 0..3 {
            let cid = Uuid::new_v4();
            db.commits()
                .insert(
                    cid,
                    Some(vm_id),
                    None,
                    owner_id,
                    format!("commit-{i}"),
                    None,
                    Utc::now(),
                    false,
                )
                .await
                .unwrap();
            commit_ids.push(cid);
        }

        let result = action::call(ListCommits::new(api_key, None, None))
            .await
            .expect("ListCommits should succeed");

        assert_eq!(result.total, 3);
        assert_eq!(result.commits.len(), 3);

        // Verify all our commits are in the result
        let returned_ids: Vec<Uuid> = result
            .commits
            .iter()
            .map(|c| c.commit_id.parse().unwrap())
            .collect();
        for cid in &commit_ids {
            assert!(
                returned_ids.contains(cid),
                "Expected commit {cid} in results"
            );
        }
    }
);

commits_test!(
    test_list_commits_omits_deleted_commits,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let api_key = get_test_api_key(env).await;
        let vm_id = insert_test_vm(env).await;
        let commit_id = Uuid::new_v4();

        db.commits()
            .insert(
                commit_id,
                Some(vm_id),
                None,
                api_key.id(),
                "temp-commit".into(),
                None,
                Utc::now(),
                false,
            )
            .await
            .unwrap();

        action::call(
            DeleteCommit::new(api_key.clone(), commit_id).skip_storage_cleanup_for_tests(),
        )
        .await
        .expect("delete should succeed");

        let result = action::call(ListCommits::new(api_key, None, None))
            .await
            .expect("ListCommits should succeed");

        assert_eq!(result.total, 0);
        assert!(result.commits.is_empty());
    }
);

commits_test!(
    test_list_commits_pagination_limit,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let api_key = get_test_api_key(env).await;
        let owner_id = api_key.id();

        let vm_id = insert_test_vm(env).await;
        for i in 0..5 {
            db.commits()
                .insert(
                    Uuid::new_v4(),
                    Some(vm_id),
                    None,
                    owner_id,
                    format!("commit-{i}"),
                    None,
                    Utc::now(),
                    false,
                )
                .await
                .unwrap();
        }

        // Request only 2
        let result = action::call(ListCommits::new(api_key, Some(2), None))
            .await
            .expect("ListCommits should succeed");

        assert_eq!(result.commits.len(), 2, "Should respect limit");
        assert_eq!(result.total, 5, "Total should reflect all commits");
        assert_eq!(result.limit, 2);
        assert_eq!(result.offset, 0);
    }
);

commits_test!(
    test_list_commits_pagination_offset,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let api_key = get_test_api_key(env).await;
        let owner_id = api_key.id();

        let vm_id = insert_test_vm(env).await;
        for i in 0..5 {
            db.commits()
                .insert(
                    Uuid::new_v4(),
                    Some(vm_id),
                    None,
                    owner_id,
                    format!("commit-{i}"),
                    None,
                    Utc::now(),
                    false,
                )
                .await
                .unwrap();
        }

        // Skip first 3
        let result = action::call(ListCommits::new(api_key, None, Some(3)))
            .await
            .expect("ListCommits should succeed");

        assert_eq!(
            result.commits.len(),
            2,
            "Should return remaining 2 after offset 3"
        );
        assert_eq!(result.total, 5);
        assert_eq!(result.offset, 3);
    }
);

commits_test!(
    test_list_commits_offset_beyond_total,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let api_key = get_test_api_key(env).await;
        let owner_id = api_key.id();

        let vm_id = insert_test_vm(env).await;
        db.commits()
            .insert(
                Uuid::new_v4(),
                Some(vm_id),
                None,
                owner_id,
                "c".into(),
                None,
                Utc::now(),
                false,
            )
            .await
            .unwrap();

        // Offset past all commits
        let result = action::call(ListCommits::new(api_key, None, Some(100)))
            .await
            .expect("ListCommits should succeed");

        assert_eq!(
            result.commits.len(),
            0,
            "Offset past total should return empty"
        );
        assert_eq!(result.total, 1, "Total should still be correct");
    }
);

commits_test!(
    test_list_commits_max_limit_clamped,
    |env: &'static ActionTestEnv| async move {
        let api_key = get_test_api_key(env).await;

        // Request limit > MAX_PAGE_SIZE (100) should be clamped
        let result = action::call(ListCommits::new(api_key, Some(999), None))
            .await
            .expect("ListCommits should succeed");

        assert_eq!(
            result.limit, 100,
            "Limit should be clamped to MAX_PAGE_SIZE"
        );
    }
);

commits_test!(
    test_list_commits_negative_values_clamped,
    |env: &'static ActionTestEnv| async move {
        let api_key = get_test_api_key(env).await;

        // Negative limit and offset should be clamped to minimums
        let result = action::call(ListCommits::new(api_key, Some(-5), Some(-10)))
            .await
            .expect("ListCommits should succeed");

        assert_eq!(result.limit, 1, "Negative limit should be clamped to 1");
        assert_eq!(result.offset, 0, "Negative offset should be clamped to 0");
    }
);

// ---------------------------------------------------------------------------
// list_commits route tests
// ---------------------------------------------------------------------------

commits_test!(
    test_route_list_commits_without_api_key,
    |env: &'static ActionTestEnv| async move {
        let client = TestClient::new(env.inbound());
        let res = client.list_commits(None, None).await;
        assert_eq!(
            res.unwrap_err(),
            orch_test::client::TestError::Unauthorized,
            "Should require authentication"
        );
    }
);

commits_test!(
    test_route_list_commits_with_api_key,
    |env: &'static ActionTestEnv| async move {
        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());
        let res = client.list_commits(None, None).await;
        assert!(res.is_ok(), "Authenticated request should succeed");
        let response = res.unwrap();
        assert_eq!(response.commits.len(), 0);
        assert_eq!(response.total, 0);
    }
);

commits_test!(
    test_route_list_commits_with_data,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let api_key = get_test_api_key(env).await;
        let owner_id = api_key.id();

        let vm_id = insert_test_vm(env).await;
        for i in 0..3 {
            db.commits()
                .insert(
                    Uuid::new_v4(),
                    Some(vm_id),
                    None,
                    owner_id,
                    format!("route-commit-{i}"),
                    Some(format!("description {i}")),
                    Utc::now(),
                    false,
                )
                .await
                .unwrap();
        }

        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());
        let res = client
            .list_commits(None, None)
            .await
            .expect("request should succeed");

        assert_eq!(res.total, 3);
        assert_eq!(res.commits.len(), 3);

        // Verify response fields are populated
        let first = &res.commits[0];
        assert!(!first.commit_id.is_empty());
        assert!(!first.owner_id.is_empty());
        assert!(!first.name.is_empty());
        assert!(!first.created_at.is_empty());
    }
);

commits_test!(
    test_route_list_commits_pagination,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let api_key = get_test_api_key(env).await;
        let owner_id = api_key.id();

        let vm_id = insert_test_vm(env).await;
        for i in 0..5 {
            db.commits()
                .insert(
                    Uuid::new_v4(),
                    Some(vm_id),
                    None,
                    owner_id,
                    format!("page-commit-{i}"),
                    None,
                    Utc::now(),
                    false,
                )
                .await
                .unwrap();
        }

        let client = TestClient::new(env.inbound()).with_bearer(env.orch_apikey());

        // Page 1: limit=2, offset=0
        let page1 = client.list_commits(Some(2), Some(0)).await.expect("page 1");
        assert_eq!(page1.commits.len(), 2);
        assert_eq!(page1.total, 5);

        // Page 2: limit=2, offset=2
        let page2 = client.list_commits(Some(2), Some(2)).await.expect("page 2");
        assert_eq!(page2.commits.len(), 2);
        assert_eq!(page2.total, 5);

        // Page 3: limit=2, offset=4
        let page3 = client.list_commits(Some(2), Some(4)).await.expect("page 3");
        assert_eq!(page3.commits.len(), 1);
        assert_eq!(page3.total, 5);

        // No overlap between pages
        let all_ids: Vec<&str> = page1
            .commits
            .iter()
            .chain(page2.commits.iter())
            .chain(page3.commits.iter())
            .map(|c| c.commit_id.as_str())
            .collect();
        let unique: std::collections::HashSet<&str> = all_ids.iter().copied().collect();
        assert_eq!(
            unique.len(),
            5,
            "All pages combined should have all 5 unique commits"
        );
    }
);

// ---------------------------------------------------------------------------
// DB-level commit repository tests
// ---------------------------------------------------------------------------

commits_test!(
    test_get_latest_by_vm_returns_most_recent,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let owner_id = seed_api_key_id();
        let vm_id = insert_test_vm(env).await;

        let first_id = Uuid::new_v4();
        let second_id = Uuid::new_v4();

        db.commits()
            .insert(
                first_id,
                Some(vm_id),
                None,
                owner_id,
                "first".into(),
                None,
                Utc::now(),
                false,
            )
            .await
            .unwrap();

        // Small delay to ensure ordering
        tokio::time::sleep(Duration::from_millis(10)).await;

        db.commits()
            .insert(
                second_id,
                Some(vm_id),
                None,
                owner_id,
                "second".into(),
                None,
                Utc::now(),
                false,
            )
            .await
            .unwrap();

        let latest = db
            .commits()
            .get_latest_by_vm(vm_id)
            .await
            .unwrap()
            .expect("Should have a latest commit");

        assert_eq!(latest.id, second_id, "Should return the most recent commit");
    }
);

commits_test!(
    test_get_latest_by_vm_returns_none_for_no_commits,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let vm_id = insert_test_vm(env).await;

        let latest = db.commits().get_latest_by_vm(vm_id).await.unwrap();
        assert!(latest.is_none(), "VM with no commits should return None");
    }
);

commits_test!(
    test_list_by_vm_returns_all_commits,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let owner_id = seed_api_key_id();
        let vm_id = insert_test_vm(env).await;
        let other_vm_id = insert_test_vm(env).await;

        // Insert commits for our VM
        for i in 0..3 {
            db.commits()
                .insert(
                    Uuid::new_v4(),
                    Some(vm_id),
                    None,
                    owner_id,
                    format!("mine-{i}"),
                    None,
                    Utc::now(),
                    false,
                )
                .await
                .unwrap();
        }

        // Insert a commit for a different VM
        db.commits()
            .insert(
                Uuid::new_v4(),
                Some(other_vm_id),
                None,
                owner_id,
                "other".into(),
                None,
                Utc::now(),
                false,
            )
            .await
            .unwrap();

        let commits = db.commits().list_by_vm(vm_id).await.unwrap();
        assert_eq!(
            commits.len(),
            3,
            "Should only return commits for the specified VM"
        );
    }
);

commits_test!(
    test_count_by_owner,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let owner_id = seed_api_key_id();
        let vm_id = insert_test_vm(env).await;

        // Initially zero
        let count = db.commits().count_by_owner(owner_id).await.unwrap();
        assert_eq!(count, 0);

        // Insert some
        for i in 0..4 {
            db.commits()
                .insert(
                    Uuid::new_v4(),
                    Some(vm_id),
                    None,
                    owner_id,
                    format!("c-{i}"),
                    None,
                    Utc::now(),
                    false,
                )
                .await
                .unwrap();
        }

        let count = db.commits().count_by_owner(owner_id).await.unwrap();
        assert_eq!(count, 4);
    }
);

// ---------------------------------------------------------------------------
// Ownership isolation
// ---------------------------------------------------------------------------

// Two different API keys create commits. ListCommits for key A must not
// return key B's commits, and vice versa.
commits_test!(
    test_list_commits_ownership_isolation,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let key_a = get_test_api_key(env).await;
        let key_b = create_second_api_key(env).await;

        let vm_id = insert_test_vm(env).await;

        // Key A creates 3 commits
        let mut key_a_commit_ids = Vec::new();
        for i in 0..3 {
            let cid = Uuid::new_v4();
            db.commits()
                .insert(
                    cid,
                    Some(vm_id),
                    None,
                    key_a.id(),
                    format!("a-{i}"),
                    None,
                    Utc::now(),
                    false,
                )
                .await
                .unwrap();
            key_a_commit_ids.push(cid);
        }

        // Key B creates 2 commits
        let mut key_b_commit_ids = Vec::new();
        for i in 0..2 {
            let cid = Uuid::new_v4();
            db.commits()
                .insert(
                    cid,
                    Some(vm_id),
                    None,
                    key_b.id(),
                    format!("b-{i}"),
                    None,
                    Utc::now(),
                    false,
                )
                .await
                .unwrap();
            key_b_commit_ids.push(cid);
        }

        // List as key A — should see only A's 3 commits
        let result_a = action::call(ListCommits::new(key_a.clone(), None, None))
            .await
            .expect("ListCommits for key A should succeed");

        assert_eq!(result_a.total, 3, "Key A should see exactly 3 commits");
        assert_eq!(result_a.commits.len(), 3);
        let returned_a_ids: Vec<Uuid> = result_a
            .commits
            .iter()
            .map(|c| c.commit_id.parse().unwrap())
            .collect();
        for cid in &key_a_commit_ids {
            assert!(
                returned_a_ids.contains(cid),
                "Key A should see its own commit {cid}"
            );
        }
        for cid in &key_b_commit_ids {
            assert!(
                !returned_a_ids.contains(cid),
                "Key A should NOT see key B's commit {cid}"
            );
        }

        // List as key B — should see only B's 2 commits
        let result_b = action::call(ListCommits::new(key_b.clone(), None, None))
            .await
            .expect("ListCommits for key B should succeed");

        assert_eq!(result_b.total, 2, "Key B should see exactly 2 commits");
        assert_eq!(result_b.commits.len(), 2);
        let returned_b_ids: Vec<Uuid> = result_b
            .commits
            .iter()
            .map(|c| c.commit_id.parse().unwrap())
            .collect();
        for cid in &key_b_commit_ids {
            assert!(
                returned_b_ids.contains(cid),
                "Key B should see its own commit {cid}"
            );
        }
        for cid in &key_a_commit_ids {
            assert!(
                !returned_b_ids.contains(cid),
                "Key B should NOT see key A's commit {cid}"
            );
        }
    }
);

// ---------------------------------------------------------------------------
// DeleteCommit action tests
// ---------------------------------------------------------------------------

commits_test!(
    test_delete_commit_removes_row,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let api_key = get_test_api_key(env).await;
        let vm_id = insert_test_vm(env).await;
        let commit_id = Uuid::new_v4();

        db.commits()
            .insert(
                commit_id,
                Some(vm_id),
                None,
                api_key.id(),
                "delete-me".into(),
                None,
                Utc::now(),
                false,
            )
            .await
            .unwrap();

        action::call(
            DeleteCommit::new(api_key.clone(), commit_id).skip_storage_cleanup_for_tests(),
        )
        .await
        .expect("DeleteCommit should succeed");

        let deleted = db.commits().get_by_id(commit_id).await.unwrap();
        assert!(deleted.is_none(), "Commit should be removed from DB");
        let (deleted_at, deleted_by) = get_commit_deleted_markers(env, commit_id).await;
        assert!(deleted_at.is_some(), "deleted_at should be recorded");
        assert_eq!(
            deleted_by,
            Some(api_key.id()),
            "deleted_by should be caller"
        );
    }
);

commits_test!(
    test_delete_commit_rejects_same_org_peer,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let owner = get_test_api_key(env).await;
        let other_owner = create_second_api_key(env).await;
        let vm_id = insert_test_vm(env).await;
        let commit_id = Uuid::new_v4();

        db.commits()
            .insert(
                commit_id,
                Some(vm_id),
                None,
                owner.id(),
                "owned-commit".into(),
                None,
                Utc::now(),
                false,
            )
            .await
            .unwrap();

        let result = action::call(
            DeleteCommit::new(other_owner, commit_id).skip_storage_cleanup_for_tests(),
        )
        .await;
        match result {
            Err(action::ActionError::Error(DeleteCommitError::Forbidden)) => {}
            other => panic!("Expected forbidden error, got {:?}", other),
        }

        let deleted = db.commits().get_by_id(commit_id).await.unwrap();
        assert!(
            deleted.is_some(),
            "Commit should remain when peer deletion is forbidden"
        );
        let (deleted_at, deleted_by) = get_commit_deleted_markers(env, commit_id).await;
        assert!(
            deleted_at.is_none(),
            "Peer attempt must not mark commit deleted"
        );
        assert!(deleted_by.is_none());
    }
);

commits_test!(
    test_delete_commit_conflicts_when_descendant_vm_exists,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let api_key = get_test_api_key(env).await;
        let parent_vm_id = insert_test_vm(env).await;
        let commit_id = Uuid::new_v4();

        db.commits()
            .insert(
                commit_id,
                Some(parent_vm_id),
                None,
                api_key.id(),
                "has-descendants".into(),
                None,
                Utc::now(),
                false,
            )
            .await
            .unwrap();

        insert_vm_from_commit(env, commit_id, Some(parent_vm_id)).await;

        let result = action::call(
            DeleteCommit::new(api_key.clone(), commit_id).skip_storage_cleanup_for_tests(),
        )
        .await;
        match result {
            Err(action::ActionError::Error(DeleteCommitError::ActiveVms(count))) => {
                assert_eq!(count, 1, "Should detect one dependent VM");
            }
            other => panic!("Expected ActiveVms conflict, got {:?}", other),
        }

        let still_exists = db.commits().get_by_id(commit_id).await.unwrap();
        assert!(
            still_exists.is_some(),
            "Commit should remain when conflict occurs"
        );
        let (deleted_at, _) = get_commit_deleted_markers(env, commit_id).await;
        assert!(
            deleted_at.is_none(),
            "Conflict should not mark commit as deleted"
        );
    }
);

commits_test!(
    test_delete_commit_forbidden_other_org,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let owner = get_test_api_key(env).await;
        let outsider = create_other_org_api_key(env).await;
        let vm_id = insert_test_vm(env).await;
        let commit_id = Uuid::new_v4();

        db.commits()
            .insert(
                commit_id,
                Some(vm_id),
                None,
                owner.id(),
                "cross-org-commit".into(),
                None,
                Utc::now(),
                false,
            )
            .await
            .unwrap();

        let result =
            action::call(DeleteCommit::new(outsider, commit_id).skip_storage_cleanup_for_tests())
                .await;
        match result {
            Err(action::ActionError::Error(DeleteCommitError::Forbidden)) => {}
            other => panic!("Expected forbidden error, got {:?}", other),
        }

        let still_exists = db.commits().get_by_id(commit_id).await.unwrap();
        assert!(
            still_exists.is_some(),
            "Commit should remain after forbidden cross-org attempt"
        );
        let (deleted_at, _) = get_commit_deleted_markers(env, commit_id).await;
        assert!(
            deleted_at.is_none(),
            "Cross-org attempt should not mark commit as deleted"
        );
    }
);

// ===========================================================================
// Public commits tests
// ===========================================================================

// ---------------------------------------------------------------------------
// DB repo: set_public, list_public, count_public
// ---------------------------------------------------------------------------

commits_test!(
    test_set_public_toggles_flag,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let owner_id = seed_api_key_id();
        let vm_id = insert_test_vm(env).await;
        let commit_id = Uuid::new_v4();

        db.commits()
            .insert(
                commit_id,
                Some(vm_id),
                None,
                owner_id,
                "priv".into(),
                None,
                Utc::now(),
                false,
            )
            .await
            .unwrap();

        // Initially private
        let c = db.commits().get_by_id(commit_id).await.unwrap().unwrap();
        assert!(!c.is_public);

        // Set public
        let updated = db.commits().set_public(commit_id, true).await.unwrap();
        assert!(updated, "set_public should return true when row exists");
        let c = db.commits().get_by_id(commit_id).await.unwrap().unwrap();
        assert!(c.is_public);

        // Set back to private
        db.commits().set_public(commit_id, false).await.unwrap();
        let c = db.commits().get_by_id(commit_id).await.unwrap().unwrap();
        assert!(!c.is_public);
    }
);

commits_test!(
    test_set_public_nonexistent_commit,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let updated = db.commits().set_public(Uuid::new_v4(), true).await.unwrap();
        assert!(
            !updated,
            "set_public should return false for nonexistent commit"
        );
    }
);

commits_test!(
    test_list_public_returns_only_public,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let owner_id = seed_api_key_id();
        let vm_id = insert_test_vm(env).await;

        // Create 3 private and 2 public commits
        for i in 0..3 {
            db.commits()
                .insert(
                    Uuid::new_v4(),
                    Some(vm_id),
                    None,
                    owner_id,
                    format!("private-{i}"),
                    None,
                    Utc::now(),
                    false,
                )
                .await
                .unwrap();
        }
        let mut public_ids = Vec::new();
        for i in 0..2 {
            let cid = Uuid::new_v4();
            db.commits()
                .insert(
                    cid,
                    Some(vm_id),
                    None,
                    owner_id,
                    format!("public-{i}"),
                    None,
                    Utc::now(),
                    true,
                )
                .await
                .unwrap();
            public_ids.push(cid);
        }

        let public_commits = db.commits().list_public(50, 0).await.unwrap();
        assert_eq!(public_commits.len(), 2);
        for c in &public_commits {
            assert!(c.is_public);
            assert!(public_ids.contains(&c.id));
        }

        let count = db.commits().count_public().await.unwrap();
        assert_eq!(count, 2);
    }
);

commits_test!(
    test_list_public_pagination,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let owner_id = seed_api_key_id();
        let vm_id = insert_test_vm(env).await;

        for i in 0..5 {
            db.commits()
                .insert(
                    Uuid::new_v4(),
                    Some(vm_id),
                    None,
                    owner_id,
                    format!("pub-{i}"),
                    None,
                    Utc::now(),
                    true,
                )
                .await
                .unwrap();
        }

        let page1 = db.commits().list_public(2, 0).await.unwrap();
        assert_eq!(page1.len(), 2);

        let page2 = db.commits().list_public(2, 2).await.unwrap();
        assert_eq!(page2.len(), 2);

        let page3 = db.commits().list_public(2, 4).await.unwrap();
        assert_eq!(page3.len(), 1);
    }
);

// ---------------------------------------------------------------------------
// Action: SetCommitPublic
// ---------------------------------------------------------------------------

commits_test!(
    test_set_commit_public_action_owner_can_toggle,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let api_key = get_test_api_key(env).await;
        let vm_id = insert_test_vm(env).await;
        let commit_id = Uuid::new_v4();

        db.commits()
            .insert(
                commit_id,
                Some(vm_id),
                None,
                api_key.id(),
                "my-commit".into(),
                None,
                Utc::now(),
                false,
            )
            .await
            .unwrap();

        // Owner sets it public
        let result = action::call(SetCommitPublic::new(commit_id, true, api_key.clone()))
            .await
            .expect("SetCommitPublic should succeed");
        assert!(result.is_public);

        // Owner sets it back to private
        let result = action::call(SetCommitPublic::new(commit_id, false, api_key))
            .await
            .expect("SetCommitPublic should succeed");
        assert!(!result.is_public);
    }
);

commits_test!(
    test_set_commit_public_action_different_org_forbidden,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let api_key = get_test_api_key(env).await;
        let other_org_key = get_second_org_api_key(env).await;
        let vm_id = insert_test_vm(env).await;
        let commit_id = Uuid::new_v4();

        db.commits()
            .insert(
                commit_id,
                Some(vm_id),
                None,
                api_key.id(),
                "my-commit".into(),
                None,
                Utc::now(),
                false,
            )
            .await
            .unwrap();

        // Other org tries to set it public — should fail
        let result = action::call(SetCommitPublic::new(commit_id, true, other_org_key)).await;
        assert!(
            result.is_err(),
            "Cross-org SetCommitPublic should be forbidden"
        );
    }
);

commits_test!(
    test_set_commit_public_action_same_org_different_key_forbidden,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let api_key = get_test_api_key(env).await;
        let other_key = create_second_api_key(env).await;
        let vm_id = insert_test_vm(env).await;
        let commit_id = Uuid::new_v4();

        // Created by api_key
        db.commits()
            .insert(
                commit_id,
                Some(vm_id),
                None,
                api_key.id(),
                "my-commit".into(),
                None,
                Utc::now(),
                false,
            )
            .await
            .unwrap();

        // Same org but different key — should fail (only the owner key can toggle)
        let result = action::call(SetCommitPublic::new(commit_id, true, other_key)).await;
        assert!(
            result.is_err(),
            "Same-org but different key should be forbidden for SetCommitPublic"
        );
    }
);

commits_test!(
    test_set_commit_public_action_not_found,
    |env: &'static ActionTestEnv| async move {
        let api_key = get_test_api_key(env).await;
        let result = action::call(SetCommitPublic::new(Uuid::new_v4(), true, api_key)).await;
        assert!(
            result.is_err(),
            "SetCommitPublic on nonexistent commit should fail"
        );
    }
);

// ---------------------------------------------------------------------------
// Action: GetCommit — public access
// ---------------------------------------------------------------------------

commits_test!(
    test_get_commit_private_same_org_allowed,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let api_key = get_test_api_key(env).await;
        let vm_id = insert_test_vm(env).await;
        let commit_id = Uuid::new_v4();

        db.commits()
            .insert(
                commit_id,
                Some(vm_id),
                None,
                api_key.id(),
                "private".into(),
                None,
                Utc::now(),
                false,
            )
            .await
            .unwrap();

        let result = action::call(GetCommit::by_id(commit_id, api_key))
            .await
            .expect("Same-org GetCommit on private commit should succeed");
        assert_eq!(result.id, commit_id);
        assert!(!result.is_public);
    }
);

commits_test!(
    test_get_commit_private_cross_org_forbidden,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let api_key = get_test_api_key(env).await;
        let other_org_key = get_second_org_api_key(env).await;
        let vm_id = insert_test_vm(env).await;
        let commit_id = Uuid::new_v4();

        db.commits()
            .insert(
                commit_id,
                Some(vm_id),
                None,
                api_key.id(),
                "private".into(),
                None,
                Utc::now(),
                false,
            )
            .await
            .unwrap();

        let result = action::call(GetCommit::by_id(commit_id, other_org_key)).await;
        assert!(
            result.is_err(),
            "Cross-org GetCommit on private commit should be forbidden"
        );
    }
);

commits_test!(
    test_get_commit_public_cross_org_allowed,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let api_key = get_test_api_key(env).await;
        let other_org_key = get_second_org_api_key(env).await;
        let vm_id = insert_test_vm(env).await;
        let commit_id = Uuid::new_v4();

        db.commits()
            .insert(
                commit_id,
                Some(vm_id),
                None,
                api_key.id(),
                "public".into(),
                None,
                Utc::now(),
                true,
            )
            .await
            .unwrap();

        let result = action::call(GetCommit::by_id(commit_id, other_org_key))
            .await
            .expect("Cross-org GetCommit on public commit should succeed");
        assert_eq!(result.id, commit_id);
        assert!(result.is_public);
    }
);

// ---------------------------------------------------------------------------
// Action: ListCommits — public mode
// ---------------------------------------------------------------------------

commits_test!(
    test_list_commits_public_mode,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let api_key = get_test_api_key(env).await;
        let other_org_key = get_second_org_api_key(env).await;
        let vm_id = insert_test_vm(env).await;

        // Create 2 private + 3 public commits
        for i in 0..2 {
            db.commits()
                .insert(
                    Uuid::new_v4(),
                    Some(vm_id),
                    None,
                    api_key.id(),
                    format!("priv-{i}"),
                    None,
                    Utc::now(),
                    false,
                )
                .await
                .unwrap();
        }
        for i in 0..3 {
            db.commits()
                .insert(
                    Uuid::new_v4(),
                    Some(vm_id),
                    None,
                    api_key.id(),
                    format!("pub-{i}"),
                    None,
                    Utc::now(),
                    true,
                )
                .await
                .unwrap();
        }

        // Public listing from the other org should see only the 3 public commits
        let result = action::call(ListCommits::public(other_org_key, None, None))
            .await
            .expect("ListCommits::public should succeed");

        assert_eq!(result.total, 3);
        assert_eq!(result.commits.len(), 3);
        for c in &result.commits {
            assert!(c.is_public);
        }
    }
);

commits_test!(
    test_list_commits_public_mode_empty,
    |env: &'static ActionTestEnv| async move {
        let api_key = get_test_api_key(env).await;

        // No public commits exist
        let result = action::call(ListCommits::public(api_key, None, None))
            .await
            .expect("ListCommits::public should succeed");

        assert_eq!(result.total, 0);
        assert_eq!(result.commits.len(), 0);
    }
);

// ---------------------------------------------------------------------------
// CommitInfo DTO includes is_public field
// ---------------------------------------------------------------------------

commits_test!(
    test_commit_info_includes_is_public,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let api_key = get_test_api_key(env).await;
        let vm_id = insert_test_vm(env).await;

        let private_id = Uuid::new_v4();
        let public_id = Uuid::new_v4();

        db.commits()
            .insert(
                private_id,
                Some(vm_id),
                None,
                api_key.id(),
                "priv".into(),
                None,
                Utc::now(),
                false,
            )
            .await
            .unwrap();
        db.commits()
            .insert(
                public_id,
                Some(vm_id),
                None,
                api_key.id(),
                "pub".into(),
                None,
                Utc::now(),
                true,
            )
            .await
            .unwrap();

        let result = action::call(ListCommits::new(api_key, None, None))
            .await
            .expect("ListCommits should succeed");

        let priv_commit = result
            .commits
            .iter()
            .find(|c| c.commit_id == private_id.to_string())
            .unwrap();
        let pub_commit = result
            .commits
            .iter()
            .find(|c| c.commit_id == public_id.to_string())
            .unwrap();

        assert!(
            !priv_commit.is_public,
            "Private commit should have is_public=false in DTO"
        );
        assert!(
            pub_commit.is_public,
            "Public commit should have is_public=true in DTO"
        );
    }
);

//! Integration tests for custom domain CRUD functionality.
//!
//! Tests cover:
//! - CreateDomain action (validation, ownership, duplicates)
//! - GetDomain action (retrieval, not found)
//! - ListDomains action (filtering by VM)
//! - DeleteDomain action (deletion, not found)
//! - Domain routes (HTTP endpoints)
//!
//! These are DB-only tests — no WireGuard or Chelsea required.
//! Run with: cargo nextest run -p orchestrator --test domains

use std::time::Duration;

use chrono::Utc;
use futures_util::FutureExt;
use orch_test::ActionTestEnv;
use orchestrator::{
    action::{self, CreateDomain, DeleteDomain, GetDomain, ListDomains},
    db::{ApiKeyEntity, ApiKeysRepository, DomainsRepository, VMsRepository},
};
use tokio::time::timeout;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Test harness helpers
// ---------------------------------------------------------------------------

macro_rules! domains_test {
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
        domains_test!($name, 15, $body);
    };
}

/// Seed data constants (from 20251111063619_seed_db.sql)
const SEED_API_KEY_ID: &str = "ef90fd52-66b5-47e7-b7dc-e73c4381028f";
const SEED_NODE_ID: &str = "4569f1fe-054b-4e8d-855a-f3545167f8a9";

fn seed_api_key_id() -> Uuid {
    Uuid::parse_str(SEED_API_KEY_ID).unwrap()
}
fn seed_node_id() -> Uuid {
    Uuid::parse_str(SEED_NODE_ID).unwrap()
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

/// Helper: insert a VM into the test DB and return its ID.
async fn insert_test_vm(env: &ActionTestEnv) -> Uuid {
    let vm_id = Uuid::new_v4();
    let ip = format!("fd00:fe11:deed:1::{}", rand_hex());
    env.db()
        .vms()
        .insert(
            vm_id,
            None,
            None,
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

/// Simple random hex suffix to avoid IP collisions in tests.
fn rand_hex() -> String {
    format!("{:x}", Uuid::new_v4().as_u128() % 0xFFFF)
}

// ---------------------------------------------------------------------------
// CreateDomain action tests
// ---------------------------------------------------------------------------

domains_test!(
    test_create_domain_success,
    |env: &'static ActionTestEnv| async move {
        let api_key = get_test_api_key(env).await;
        let vm_id = insert_test_vm(env).await;

        let result = action::call(CreateDomain::new(
            vm_id,
            "example.com".to_string(),
            api_key.clone(),
        ))
        .await
        .expect("CreateDomain should succeed");

        assert_eq!(result.domain, "example.com");
        assert_eq!(result.vm_id, vm_id);
        assert!(!result.domain_id.is_nil());
    }
);

domains_test!(
    test_create_domain_lowercases_input,
    |env: &'static ActionTestEnv| async move {
        let api_key = get_test_api_key(env).await;
        let vm_id = insert_test_vm(env).await;

        let result = action::call(CreateDomain::new(
            vm_id,
            "EXAMPLE.COM".to_string(),
            api_key.clone(),
        ))
        .await
        .expect("CreateDomain should succeed");

        assert_eq!(result.domain, "example.com", "Domain should be lowercased");
    }
);

domains_test!(
    test_create_domain_invalid_format,
    |env: &'static ActionTestEnv| async move {
        let api_key = get_test_api_key(env).await;
        let vm_id = insert_test_vm(env).await;

        // Missing TLD
        let result = action::call(CreateDomain::new(
            vm_id,
            "localhost".to_string(),
            api_key.clone(),
        ))
        .await;

        assert!(result.is_err(), "Should reject single-label domain");

        // Email-like format
        let result = action::call(CreateDomain::new(
            vm_id,
            "user@example.com".to_string(),
            api_key.clone(),
        ))
        .await;

        assert!(result.is_err(), "Should reject email-like domain");

        // Numeric TLD
        let result = action::call(CreateDomain::new(
            vm_id,
            "example.123".to_string(),
            api_key.clone(),
        ))
        .await;

        assert!(result.is_err(), "Should reject numeric TLD");
    }
);

domains_test!(
    test_create_domain_vm_not_found,
    |env: &'static ActionTestEnv| async move {
        let api_key = get_test_api_key(env).await;
        let nonexistent_vm_id = Uuid::new_v4();

        let result = action::call(CreateDomain::new(
            nonexistent_vm_id,
            "example.com".to_string(),
            api_key.clone(),
        ))
        .await;

        assert!(result.is_err(), "Should fail for non-existent VM");
    }
);

domains_test!(
    test_create_domain_duplicate_rejected,
    |env: &'static ActionTestEnv| async move {
        let api_key = get_test_api_key(env).await;
        let vm_id = insert_test_vm(env).await;

        // Create first domain
        action::call(CreateDomain::new(
            vm_id,
            "unique-domain.com".to_string(),
            api_key.clone(),
        ))
        .await
        .expect("First creation should succeed");

        // Try to create the same domain again
        let result = action::call(CreateDomain::new(
            vm_id,
            "unique-domain.com".to_string(),
            api_key.clone(),
        ))
        .await;

        assert!(result.is_err(), "Duplicate domain should be rejected");
    }
);

domains_test!(
    test_create_domain_duplicate_case_insensitive,
    |env: &'static ActionTestEnv| async move {
        let api_key = get_test_api_key(env).await;
        let vm_id = insert_test_vm(env).await;

        action::call(CreateDomain::new(
            vm_id,
            "case-test.com".to_string(),
            api_key.clone(),
        ))
        .await
        .expect("First creation should succeed");

        // Try with different case
        let result = action::call(CreateDomain::new(
            vm_id,
            "CASE-TEST.COM".to_string(),
            api_key.clone(),
        ))
        .await;

        assert!(
            result.is_err(),
            "Case-insensitive duplicate should be rejected"
        );
    }
);

// ---------------------------------------------------------------------------
// GetDomain action tests
// ---------------------------------------------------------------------------

domains_test!(
    test_get_domain_success,
    |env: &'static ActionTestEnv| async move {
        let api_key = get_test_api_key(env).await;
        let vm_id = insert_test_vm(env).await;

        let created = action::call(CreateDomain::new(
            vm_id,
            "get-test.example.com".to_string(),
            api_key.clone(),
        ))
        .await
        .expect("CreateDomain should succeed");

        let result = action::call(GetDomain::new(created.domain_id, api_key.clone()))
            .await
            .expect("GetDomain should succeed");

        assert_eq!(result.domain_id, created.domain_id);
        assert_eq!(result.domain, "get-test.example.com");
        assert_eq!(result.vm_id, vm_id);
    }
);

domains_test!(
    test_get_domain_not_found,
    |env: &'static ActionTestEnv| async move {
        let api_key = get_test_api_key(env).await;
        let nonexistent_id = Uuid::new_v4();

        let result = action::call(GetDomain::new(nonexistent_id, api_key.clone())).await;

        assert!(result.is_err(), "Should fail for non-existent domain");
    }
);

// ---------------------------------------------------------------------------
// ListDomains action tests
// ---------------------------------------------------------------------------

domains_test!(
    test_list_domains_empty,
    |env: &'static ActionTestEnv| async move {
        let api_key = get_test_api_key(env).await;

        let result = action::call(ListDomains::new(None, api_key.clone()))
            .await
            .expect("ListDomains should succeed");

        assert_eq!(result.len(), 0);
    }
);

domains_test!(
    test_list_domains_returns_owned_domains,
    |env: &'static ActionTestEnv| async move {
        let api_key = get_test_api_key(env).await;
        let vm_id = insert_test_vm(env).await;

        // Create some domains
        for i in 0..3 {
            action::call(CreateDomain::new(
                vm_id,
                format!("list-test-{i}.example.com"),
                api_key.clone(),
            ))
            .await
            .expect("CreateDomain should succeed");
        }

        let result = action::call(ListDomains::new(None, api_key.clone()))
            .await
            .expect("ListDomains should succeed");

        assert_eq!(result.len(), 3);
    }
);

domains_test!(
    test_list_domains_filters_by_vm,
    |env: &'static ActionTestEnv| async move {
        let api_key = get_test_api_key(env).await;
        let vm1 = insert_test_vm(env).await;
        let vm2 = insert_test_vm(env).await;

        // Create domains on vm1
        for i in 0..2 {
            action::call(CreateDomain::new(
                vm1,
                format!("vm1-domain-{i}.example.com"),
                api_key.clone(),
            ))
            .await
            .expect("CreateDomain should succeed");
        }

        // Create domains on vm2
        for i in 0..3 {
            action::call(CreateDomain::new(
                vm2,
                format!("vm2-domain-{i}.example.com"),
                api_key.clone(),
            ))
            .await
            .expect("CreateDomain should succeed");
        }

        // List for vm1 only
        let result = action::call(ListDomains::new(Some(vm1), api_key.clone()))
            .await
            .expect("ListDomains should succeed");

        assert_eq!(result.len(), 2, "Should only return vm1's domains");
        for domain in &result {
            assert_eq!(domain.vm_id, vm1);
        }
    }
);

// ---------------------------------------------------------------------------
// DeleteDomain action tests
// ---------------------------------------------------------------------------

domains_test!(
    test_delete_domain_success,
    |env: &'static ActionTestEnv| async move {
        let api_key = get_test_api_key(env).await;
        let vm_id = insert_test_vm(env).await;

        let created = action::call(CreateDomain::new(
            vm_id,
            "delete-test.example.com".to_string(),
            api_key.clone(),
        ))
        .await
        .expect("CreateDomain should succeed");

        let deleted_id = action::call(DeleteDomain::new(created.domain_id, api_key.clone()))
            .await
            .expect("DeleteDomain should succeed");

        assert_eq!(deleted_id, created.domain_id);

        // Verify it's gone
        let get_result = action::call(GetDomain::new(created.domain_id, api_key.clone())).await;
        assert!(get_result.is_err(), "Domain should no longer exist");
    }
);

domains_test!(
    test_delete_domain_not_found,
    |env: &'static ActionTestEnv| async move {
        let api_key = get_test_api_key(env).await;
        let nonexistent_id = Uuid::new_v4();

        let result = action::call(DeleteDomain::new(nonexistent_id, api_key.clone())).await;

        assert!(result.is_err(), "Should fail for non-existent domain");
    }
);

// ---------------------------------------------------------------------------
// DB-level repository tests
// ---------------------------------------------------------------------------

domains_test!(
    test_repo_insert_and_get_by_id,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let vm_id = insert_test_vm(env).await;
        let owner_id = seed_api_key_id();

        let entity = db
            .domains()
            .insert(owner_id, vm_id, "repo-test.example.com")
            .await
            .expect("Insert should succeed");

        assert_eq!(entity.domain(), "repo-test.example.com");
        assert_eq!(entity.vm_id(), vm_id);
        assert_eq!(entity.owner_id(), owner_id);

        let fetched = db
            .domains()
            .get_by_id(entity.domain_id())
            .await
            .expect("Get should succeed")
            .expect("Domain should exist");

        assert_eq!(fetched.domain_id(), entity.domain_id());
    }
);

domains_test!(
    test_repo_get_by_domain,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let vm_id = insert_test_vm(env).await;
        let owner_id = seed_api_key_id();

        db.domains()
            .insert(owner_id, vm_id, "by-domain-test.example.com")
            .await
            .expect("Insert should succeed");

        let fetched = db
            .domains()
            .get_by_domain("by-domain-test.example.com")
            .await
            .expect("Get should succeed")
            .expect("Domain should exist");

        assert_eq!(fetched.domain(), "by-domain-test.example.com");
    }
);

domains_test!(
    test_repo_list_by_owner,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let vm_id = insert_test_vm(env).await;
        let owner_id = seed_api_key_id();

        for i in 0..3 {
            db.domains()
                .insert(owner_id, vm_id, &format!("owner-list-{i}.example.com"))
                .await
                .expect("Insert should succeed");
        }

        let list = db
            .domains()
            .list_by_owner(owner_id)
            .await
            .expect("List should succeed");

        assert_eq!(list.len(), 3);
    }
);

domains_test!(
    test_repo_list_by_vm,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let vm1 = insert_test_vm(env).await;
        let vm2 = insert_test_vm(env).await;
        let owner_id = seed_api_key_id();

        db.domains()
            .insert(owner_id, vm1, "vm-list-1.example.com")
            .await
            .expect("Insert should succeed");
        db.domains()
            .insert(owner_id, vm2, "vm-list-2.example.com")
            .await
            .expect("Insert should succeed");

        let list = db
            .domains()
            .list_by_vm(vm1)
            .await
            .expect("List should succeed");

        assert_eq!(list.len(), 1);
        assert_eq!(list[0].vm_id(), vm1);
    }
);

domains_test!(test_repo_delete, |env: &'static ActionTestEnv| async move {
    let db = env.db();
    let vm_id = insert_test_vm(env).await;
    let owner_id = seed_api_key_id();

    let entity = db
        .domains()
        .insert(owner_id, vm_id, "delete-repo.example.com")
        .await
        .expect("Insert should succeed");

    let deleted = db
        .domains()
        .delete(entity.domain_id())
        .await
        .expect("Delete should succeed");

    assert!(deleted, "Delete should return true");

    let fetched = db
        .domains()
        .get_by_id(entity.domain_id())
        .await
        .expect("Get should succeed");

    assert!(fetched.is_none(), "Domain should no longer exist");
});

domains_test!(
    test_repo_delete_nonexistent,
    |env: &'static ActionTestEnv| async move {
        let db = env.db();
        let nonexistent_id = Uuid::new_v4();

        let deleted = db
            .domains()
            .delete(nonexistent_id)
            .await
            .expect("Delete should not error");

        assert!(
            !deleted,
            "Delete should return false for nonexistent domain"
        );
    }
);

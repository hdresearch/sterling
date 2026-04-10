//! Integration tests for API key CRUD operations.
//! Run with: cargo nextest run -p llm_proxy --test key_management

mod test_harness;

use llm_proxy::auth;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use test_harness::TestEnv;
use uuid::Uuid;

#[test]
fn create_and_retrieve_key() {
    TestEnv::with_env(|env| async move {
        let (raw_key, key_hash, key_prefix) = auth::generate_api_key();
        let id = Uuid::new_v4();

        env.billing
            .create_api_key(
                id,
                &key_hash,
                &key_prefix,
                Some("test-key"),
                None,
                None,
                None,
                &[],
                None,
            )
            .await
            .unwrap();

        // Retrieve by hash
        let row = env
            .billing
            .get_api_key_by_hash(&key_hash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.id, id);
        assert_eq!(row.key_prefix, key_prefix);
        assert_eq!(row.name.as_deref(), Some("test-key"));
        assert_eq!(row.spend, Decimal::ZERO);
        assert!(!row.revoked);

        // Verify raw key hashes to the same thing
        assert_eq!(auth::hash_api_key(&raw_key), key_hash);
    });
}

#[test]
fn unknown_key_returns_none() {
    TestEnv::with_env(|env| async move {
        let result = env
            .billing
            .get_api_key_by_hash("nonexistent_hash_value")
            .await
            .unwrap();
        assert!(result.is_none());
    });
}

#[test]
fn revoke_key() {
    TestEnv::with_env(|env| async move {
        let (_, key_hash, key_prefix) = auth::generate_api_key();
        let id = Uuid::new_v4();

        env.billing
            .create_api_key(
                id,
                &key_hash,
                &key_prefix,
                None,
                None,
                None,
                None,
                &[],
                None,
            )
            .await
            .unwrap();

        // Revoke it
        let revoked = env.billing.revoke_api_key(id).await.unwrap();
        assert!(revoked);

        // Verify it's revoked
        let row = env
            .billing
            .get_api_key_by_hash(&key_hash)
            .await
            .unwrap()
            .unwrap();
        assert!(row.revoked);

        // Revoking again returns false
        let revoked_again = env.billing.revoke_api_key(id).await.unwrap();
        assert!(!revoked_again);
    });
}

#[test]
fn revoke_nonexistent_key() {
    TestEnv::with_env(|env| async move {
        let result = env.billing.revoke_api_key(Uuid::new_v4()).await.unwrap();
        assert!(!result);
    });
}

#[test]
fn list_keys_all() {
    TestEnv::with_env(|env| async move {
        // Create 3 keys
        for i in 0..3 {
            let (_, key_hash, key_prefix) = auth::generate_api_key();
            env.billing
                .create_api_key(
                    Uuid::new_v4(),
                    &key_hash,
                    &key_prefix,
                    Some(&format!("key-{i}")),
                    None,
                    None,
                    None,
                    &[],
                    None,
                )
                .await
                .unwrap();
        }

        let keys = env.billing.list_api_keys(None).await.unwrap();
        assert_eq!(keys.len(), 3);
    });
}

#[test]
fn list_keys_by_team() {
    TestEnv::with_env(|env| async move {
        let team_a = Uuid::new_v4();
        let team_b = Uuid::new_v4();

        // Create teams first
        env.billing
            .create_team(team_a, "Team A", None)
            .await
            .unwrap();
        env.billing
            .create_team(team_b, "Team B", None)
            .await
            .unwrap();

        // 2 keys for team A, 1 for team B
        for _ in 0..2 {
            let (_, hash, prefix) = auth::generate_api_key();
            env.billing
                .create_api_key(
                    Uuid::new_v4(),
                    &hash,
                    &prefix,
                    None,
                    None,
                    Some(team_a),
                    None,
                    &[],
                    None,
                )
                .await
                .unwrap();
        }
        let (_, hash, prefix) = auth::generate_api_key();
        env.billing
            .create_api_key(
                Uuid::new_v4(),
                &hash,
                &prefix,
                None,
                None,
                Some(team_b),
                None,
                &[],
                None,
            )
            .await
            .unwrap();

        let team_a_keys = env.billing.list_api_keys(Some(team_a)).await.unwrap();
        assert_eq!(team_a_keys.len(), 2);

        let team_b_keys = env.billing.list_api_keys(Some(team_b)).await.unwrap();
        assert_eq!(team_b_keys.len(), 1);
    });
}

#[test]
fn key_with_budget_and_models() {
    TestEnv::with_env(|env| async move {
        let (_, key_hash, key_prefix) = auth::generate_api_key();
        let id = Uuid::new_v4();
        let models = vec!["gpt-4o".to_string(), "claude-sonnet".to_string()];

        env.billing
            .create_api_key(
                id,
                &key_hash,
                &key_prefix,
                Some("budgeted"),
                None,
                None,
                Some(dec!(100.0)),
                &models,
                None,
            )
            .await
            .unwrap();

        let row = env
            .billing
            .get_api_key_by_hash(&key_hash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.max_budget, Some(dec!(100.0)));
        assert_eq!(row.models, models);
    });
}

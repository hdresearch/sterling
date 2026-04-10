//! Integration tests for budget enforcement.
//! Run with: cargo nextest run -p llm_proxy --test budget_enforcement

mod test_harness;

use llm_proxy::auth;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use test_harness::TestEnv;
use uuid::Uuid;

/// Create a key assigned to a funded team so team-level credit checks pass.
/// Per-key max_budget is what these tests exercise.
async fn create_key(env: &TestEnv, max_budget: Option<Decimal>) -> (Uuid, String) {
    let team = env.create_funded_team(dec!(1000.0)).await;
    let (_, key_hash, key_prefix) = auth::generate_api_key();
    let key_id = Uuid::new_v4();
    env.billing
        .create_api_key(
            key_id,
            &key_hash,
            &key_prefix,
            None,
            None,
            Some(team.id),
            max_budget,
            &[],
            None,
        )
        .await
        .unwrap();
    (key_id, key_hash)
}

#[test]
fn key_under_budget_can_query() {
    TestEnv::with_env(|env| async move {
        let (key_id, key_hash) = create_key(env, Some(dec!(10.0))).await;
        env.billing
            .increment_key_spend(key_id, dec!(5.0))
            .await
            .unwrap();

        let row = env
            .billing
            .get_api_key_by_hash(&key_hash)
            .await
            .unwrap()
            .unwrap();
        assert!(row.spend < row.max_budget.unwrap());
        assert!(row.deny_reason.is_none());
    });
}

#[test]
fn key_at_budget_is_blocked() {
    TestEnv::with_env(|env| async move {
        let (key_id, key_hash) = create_key(env, Some(dec!(10.0))).await;
        env.billing
            .increment_key_spend(key_id, dec!(10.0))
            .await
            .unwrap();

        let row = env
            .billing
            .get_api_key_by_hash(&key_hash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.deny_reason.as_deref(), Some("budget_exceeded"));
    });
}

#[test]
fn key_over_budget_is_blocked() {
    TestEnv::with_env(|env| async move {
        let (key_id, key_hash) = create_key(env, Some(dec!(5.0))).await;
        env.billing
            .increment_key_spend(key_id, dec!(7.0))
            .await
            .unwrap();

        let row = env
            .billing
            .get_api_key_by_hash(&key_hash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.deny_reason.as_deref(), Some("budget_exceeded"));
    });
}

#[test]
fn key_with_no_credits_and_no_budget_is_blocked() {
    TestEnv::with_env(|env| async move {
        // Key with no team → no_credits
        let (_, key_hash, key_prefix) = auth::generate_api_key();
        let key_id = Uuid::new_v4();
        env.billing
            .create_api_key(
                key_id,
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

        let row = env
            .billing
            .get_api_key_by_hash(&key_hash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.deny_reason.as_deref(), Some("no_credits"));
    });
}

#[test]
fn key_with_credits_can_query() {
    TestEnv::with_env(|env| async move {
        // Key in a funded team with no per-key budget → allowed
        let team = env.create_funded_team(dec!(50.0)).await;
        let (_, key_hash, key_prefix) = auth::generate_api_key();
        let key_id = Uuid::new_v4();
        env.billing
            .create_api_key(
                key_id,
                &key_hash,
                &key_prefix,
                None,
                None,
                Some(team.id),
                None,
                &[],
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
        assert!(row.deny_reason.is_none());
    });
}

#[test]
fn expired_key_is_blocked() {
    TestEnv::with_env(|env| async move {
        let (_, key_hash, key_prefix) = auth::generate_api_key();
        let key_id = Uuid::new_v4();
        let yesterday = chrono::Utc::now() - chrono::Duration::days(1);
        env.billing
            .create_api_key(
                key_id,
                &key_hash,
                &key_prefix,
                None,
                None,
                None,
                None,
                &[],
                Some(yesterday),
            )
            .await
            .unwrap();

        let row = env
            .billing
            .get_api_key_by_hash(&key_hash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.deny_reason.as_deref(), Some("key_expired"));
    });
}

#[test]
fn increase_budget_unblocks_key() {
    TestEnv::with_env(|env| async move {
        let (key_id, key_hash) = create_key(env, Some(dec!(5.0))).await;
        env.billing
            .increment_key_spend(key_id, dec!(6.0))
            .await
            .unwrap();

        let row = env
            .billing
            .get_api_key_by_hash(&key_hash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.deny_reason.as_deref(), Some("budget_exceeded"));

        env.billing
            .update_api_key_budget(key_id, Some(dec!(50.0)), false)
            .await
            .unwrap();

        let row = env
            .billing
            .get_api_key_by_hash(&key_hash)
            .await
            .unwrap()
            .unwrap();
        assert!(row.deny_reason.is_none());
    });
}

#[test]
fn reset_spend_to_zero() {
    TestEnv::with_env(|env| async move {
        let (key_id, key_hash) = create_key(env, Some(dec!(10.0))).await;
        env.billing
            .increment_key_spend(key_id, dec!(8.0))
            .await
            .unwrap();

        let row = env
            .billing
            .get_api_key_by_hash(&key_hash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.spend, dec!(8.0));

        env.billing
            .update_api_key_budget(key_id, Some(dec!(10.0)), true)
            .await
            .unwrap();

        let row = env
            .billing
            .get_api_key_by_hash(&key_hash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.spend, Decimal::ZERO);
        assert_eq!(row.max_budget, Some(dec!(10.0)));
    });
}

#[test]
fn model_access_restriction() {
    TestEnv::with_env(|env| async move {
        let team = env.create_funded_team(dec!(100.0)).await;
        let (_, key_hash, key_prefix) = auth::generate_api_key();
        let key_id = Uuid::new_v4();
        let models = vec!["gpt-4o".to_string()];
        env.billing
            .create_api_key(
                key_id,
                &key_hash,
                &key_prefix,
                None,
                None,
                Some(team.id),
                None,
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
        assert!(row.models.contains(&"gpt-4o".to_string()));
        assert!(!row.models.contains(&"claude-sonnet".to_string()));
    });
}

//! Integration tests for the prepaid credits system.
//! Run with: cargo nextest run -p llm_proxy --test credits

mod test_harness;

use llm_proxy::auth;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use test_harness::TestEnv;
use uuid::Uuid;

/// Create a key in a funded team. Returns (key_id, key_hash, team_id).
async fn create_key(env: &TestEnv) -> (Uuid, String, Uuid) {
    let team = env.create_funded_team(dec!(0)).await;
    let (_, key_hash, key_prefix) = auth::generate_api_key();
    let id = Uuid::new_v4();
    env.billing
        .create_api_key(
            id,
            &key_hash,
            &key_prefix,
            Some("test"),
            None,
            Some(team.id),
            None,
            &[],
            None,
        )
        .await
        .unwrap();
    (id, key_hash, team.id)
}

#[test]
fn add_credits_to_team() {
    TestEnv::with_env(|env| async move {
        let (_key_id, key_hash, team_id) = create_key(env).await;

        let balance = env
            .billing
            .add_team_credits(team_id, dec!(50.0), "initial top-up", None, "admin")
            .await
            .unwrap();
        assert_eq!(balance, dec!(50.0));

        // Auth query reads team-level credits
        let row = env
            .billing
            .get_api_key_by_hash(&key_hash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.credits, dec!(50.0));
    });
}

#[test]
fn multiple_topups_accumulate() {
    TestEnv::with_env(|env| async move {
        let (_key_id, key_hash, team_id) = create_key(env).await;

        env.billing
            .add_team_credits(team_id, dec!(20.0), "first top-up", None, "admin")
            .await
            .unwrap();
        env.billing
            .add_team_credits(
                team_id,
                dec!(30.0),
                "second top-up",
                Some("stripe_pi_123"),
                "stripe",
            )
            .await
            .unwrap();
        let balance = env
            .billing
            .add_team_credits(team_id, dec!(10.0), "bonus", None, "system")
            .await
            .unwrap();

        assert_eq!(balance, dec!(60.0));

        let row = env
            .billing
            .get_api_key_by_hash(&key_hash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.credits, dec!(60.0));
    });
}

#[test]
fn credits_minus_spend_is_remaining() {
    TestEnv::with_env(|env| async move {
        let (_key_id, key_hash, team_id) = create_key(env).await;

        env.billing
            .add_team_credits(team_id, dec!(50.0), "top-up", None, "admin")
            .await
            .unwrap();
        // Spend is tracked at team level for credit checks
        env.billing
            .increment_team_spend(team_id, dec!(15.0))
            .await
            .unwrap();

        let row = env
            .billing
            .get_api_key_by_hash(&key_hash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.credits, dec!(50.0));
        // Note: row.spend is per-key spend (0), team spend is what matters for deny_reason
        assert!(row.deny_reason.is_none());
    });
}

#[test]
fn blocked_when_credits_exhausted() {
    TestEnv::with_env(|env| async move {
        let (_key_id, key_hash, team_id) = create_key(env).await;

        env.billing
            .add_team_credits(team_id, dec!(10.0), "top-up", None, "admin")
            .await
            .unwrap();
        env.billing
            .increment_team_spend(team_id, dec!(12.0))
            .await
            .unwrap();

        let row = env
            .billing
            .get_api_key_by_hash(&key_hash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.deny_reason.as_deref(), Some("credits_exhausted"));
    });
}

#[test]
fn topup_unblocks_exhausted_key() {
    TestEnv::with_env(|env| async move {
        let (_key_id, key_hash, team_id) = create_key(env).await;

        env.billing
            .add_team_credits(team_id, dec!(10.0), "initial", None, "admin")
            .await
            .unwrap();
        env.billing
            .increment_team_spend(team_id, dec!(12.0))
            .await
            .unwrap();

        let row = env
            .billing
            .get_api_key_by_hash(&key_hash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.deny_reason.as_deref(), Some("credits_exhausted"));

        env.billing
            .add_team_credits(team_id, dec!(50.0), "top-up", None, "stripe")
            .await
            .unwrap();

        let row = env
            .billing
            .get_api_key_by_hash(&key_hash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.credits, dec!(60.0));
        assert!(row.deny_reason.is_none());
    });
}

#[test]
fn credit_history_is_recorded() {
    TestEnv::with_env(|env| async move {
        let (key_id, _, _team_id) = create_key(env).await;

        env.billing
            .add_credits(
                key_id,
                dec!(25.0),
                "first top-up",
                Some("stripe_pi_aaa"),
                "stripe",
            )
            .await
            .unwrap();
        env.billing
            .add_credits(key_id, dec!(10.0), "bonus", None, "admin")
            .await
            .unwrap();

        let history = env.billing.get_credit_history(key_id).await.unwrap();
        assert_eq!(history.len(), 2);

        assert_eq!(history[0].amount, dec!(10.0));
        assert_eq!(history[0].description, "bonus");
        assert_eq!(history[0].balance_after, dec!(35.0));

        assert_eq!(history[1].amount, dec!(25.0));
        assert_eq!(history[1].description, "first top-up");
        assert_eq!(history[1].reference_id.as_deref(), Some("stripe_pi_aaa"));
        assert_eq!(history[1].balance_after, dec!(25.0));
    });
}

#[test]
fn negative_credit_amount_rejected() {
    TestEnv::with_env(|env| async move {
        let (key_id, _, _team_id) = create_key(env).await;

        let result = env
            .billing
            .add_credits(key_id, dec!(-10.0), "negative", None, "admin")
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must be positive"));
    });
}

#[test]
fn zero_credits_team_with_no_budget_is_blocked() {
    TestEnv::with_env(|env| async move {
        // Team with 0 credits and no max_budget → no_credits
        let (_key_id, key_hash, _team_id) = create_key(env).await;

        let row = env
            .billing
            .get_api_key_by_hash(&key_hash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.credits, Decimal::ZERO);
        assert!(row.max_budget.is_none());
        assert_eq!(row.deny_reason.as_deref(), Some("no_credits"));
    });
}

//! Integration tests for spend tracking (split across BillingDb and LogDb).
//! Run with: cargo nextest run -p llm_proxy --test spend_tracking

mod test_harness;

use llm_proxy::auth;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::json;
use test_harness::TestEnv;
use uuid::Uuid;

/// Helper: create a key in a funded team and return (id, hash, team_id)
async fn create_test_key(env: &TestEnv) -> (Uuid, String, Uuid) {
    let team = env.create_funded_team(dec!(1000.0)).await;
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

/// Helper: record a request to LogDb and update spend in BillingDb (mirrors prod flow)
async fn record_and_bill(
    env: &TestEnv,
    key_id: Uuid,
    team_id: Option<Uuid>,
    model: &str,
    provider: &str,
    prompt_tokens: i32,
    completion_tokens: i32,
    spend: Decimal,
    duration_ms: i32,
    req_body: &serde_json::Value,
    resp_body: &serde_json::Value,
) -> Uuid {
    let request_id = Uuid::new_v4();
    env.logs
        .record_request(
            request_id,
            key_id,
            team_id,
            model,
            provider,
            prompt_tokens,
            completion_tokens,
            spend,
            duration_ms,
            "success",
            Some("end_turn"),
            None,
            req_body,
            resp_body,
        )
        .await
        .unwrap();
    env.billing
        .increment_key_spend(key_id, spend)
        .await
        .unwrap();
    if let Some(tid) = team_id {
        env.billing.increment_team_spend(tid, spend).await.unwrap();
    }
    request_id
}

#[test]
fn record_request_updates_key_spend() {
    TestEnv::with_env(|env| async move {
        let (key_id, key_hash, _team_id) = create_test_key(env).await;

        record_and_bill(
            env,
            key_id,
            None,
            "gpt-4o",
            "openai",
            1000,
            500,
            dec!(0.00075),
            1500,
            &json!({"model": "gpt-4o", "messages": []}),
            &json!({"choices": []}),
        )
        .await;

        let row = env
            .billing
            .get_api_key_by_hash(&key_hash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.spend, dec!(0.00075));
    });
}

#[test]
fn multiple_requests_accumulate_spend() {
    TestEnv::with_env(|env| async move {
        let (key_id, key_hash, _team_id) = create_test_key(env).await;

        for i in 0..5 {
            record_and_bill(
                env,
                key_id,
                None,
                "gpt-4o",
                "openai",
                100,
                50,
                dec!(0.001),
                200,
                &json!({"request": i}),
                &json!({"response": i}),
            )
            .await;
        }

        let row = env
            .billing
            .get_api_key_by_hash(&key_hash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.spend, dec!(0.005));
    });
}

#[test]
fn team_spend_tracking() {
    TestEnv::with_env(|env| async move {
        let team_id = Uuid::new_v4();
        env.billing
            .create_team(team_id, "Test Team", None)
            .await
            .unwrap();

        let (_, key_hash, key_prefix) = auth::generate_api_key();
        let key_id = Uuid::new_v4();
        env.billing
            .create_api_key(
                key_id,
                &key_hash,
                &key_prefix,
                None,
                None,
                Some(team_id),
                None,
                &[],
                None,
            )
            .await
            .unwrap();

        record_and_bill(
            env,
            key_id,
            Some(team_id),
            "gpt-4o",
            "openai",
            1000,
            500,
            dec!(1.50),
            2000,
            &json!({}),
            &json!({}),
        )
        .await;

        let team = env.billing.get_team(team_id).await.unwrap().unwrap();
        assert_eq!(team.spend, dec!(1.50));
    });
}

#[test]
fn query_spend_by_key() {
    TestEnv::with_env(|env| async move {
        let (key_id, _, _team_id) = create_test_key(env).await;

        for _ in 0..3 {
            record_and_bill(
                env,
                key_id,
                None,
                "gpt-4o",
                "openai",
                500,
                200,
                dec!(0.50),
                1000,
                &json!({}),
                &json!({}),
            )
            .await;
        }

        let summary = env.logs.get_spend_by_key(key_id, None).await.unwrap();
        assert_eq!(summary.request_count, 3);
        assert_eq!(summary.total_spend, dec!(1.50));
        assert_eq!(summary.total_prompt_tokens, 1500);
        assert_eq!(summary.total_completion_tokens, 600);
    });
}

#[test]
fn query_spend_by_model() {
    TestEnv::with_env(|env| async move {
        let (key_id, _, _team_id) = create_test_key(env).await;

        for _ in 0..2 {
            record_and_bill(
                env,
                key_id,
                None,
                "gpt-4o",
                "openai",
                100,
                50,
                dec!(0.10),
                500,
                &json!({}),
                &json!({}),
            )
            .await;
        }

        record_and_bill(
            env,
            key_id,
            None,
            "claude-sonnet",
            "anthropic",
            200,
            100,
            dec!(0.50),
            800,
            &json!({}),
            &json!({}),
        )
        .await;

        let models = env.logs.get_spend_by_model(None).await.unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].model, "claude-sonnet");
        assert_eq!(models[0].total_spend, dec!(0.50));
        assert_eq!(models[1].model, "gpt-4o");
        assert_eq!(models[1].total_spend, dec!(0.20));
    });
}

#[test]
fn request_logs_stored_with_payloads() {
    TestEnv::with_env(|env| async move {
        let (key_id, _, _team_id) = create_test_key(env).await;

        let req_body = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hello"}]
        });
        let resp_body = json!({
            "choices": [{"message": {"content": "hi"}}]
        });

        let request_id = record_and_bill(
            env,
            key_id,
            None,
            "gpt-4o",
            "openai",
            10,
            5,
            dec!(0.001),
            200,
            &req_body,
            &resp_body,
        )
        .await;

        let log = env.logs.get_request_log(request_id).await.unwrap().unwrap();
        assert_eq!(log.model, "gpt-4o");
        assert_eq!(log.request_body, req_body);
        assert_eq!(log.response_body, resp_body);
        assert_eq!(log.stop_reason.as_deref(), Some("end_turn"));
    });
}

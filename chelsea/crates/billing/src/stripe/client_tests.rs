//! Tests for StripeClient methods against mock HTTP server.

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::stripe::client::StripeClient;
    use crate::stripe::test_helpers::MockStripeServer;

    // ─── Credit Balance ──────────────────────────────────────────────

    #[tokio::test]
    async fn get_credit_balance_sums_balances() {
        let mock = MockStripeServer::start().await;
        mock.on(
            "/v1/billing/credit_balance_summary",
            json!({
                "balances": [
                    {"available_balance": {"monetary": {"value": 5000, "currency": "usd"}}},
                    {"available_balance": {"monetary": {"value": 3000, "currency": "usd"}}}
                ]
            }),
        );

        let balance = mock.client().get_credit_balance("cus_123").await.unwrap();
        assert_eq!(balance, 8000);
    }

    #[tokio::test]
    async fn get_credit_balance_empty() {
        let mock = MockStripeServer::start().await;
        mock.on(
            "/v1/billing/credit_balance_summary",
            json!({"balances": []}),
        );

        let balance = mock.client().get_credit_balance("cus_empty").await.unwrap();
        assert_eq!(balance, 0);
    }

    // ─── Credit Grants ───────────────────────────────────────────────

    #[tokio::test]
    async fn create_credit_grant_sends_correct_params() {
        let mock = MockStripeServer::start().await;
        mock.on("/v1/billing/credit_grants", json!({"id": "cg_test"}));

        mock.client()
            .create_credit_grant(
                "cus_abc",
                5000,
                "paid",
                &[("org_id", "org-1"), ("reason", "test top-up")],
            )
            .await
            .unwrap();

        let reqs = mock.recorded();
        let req = reqs
            .iter()
            .find(|(_, path, _)| path.contains("credit_grants"))
            .unwrap();
        assert_eq!(req.0, "POST");
        assert!(req.2.contains("cus_abc"));
        assert!(req.2.contains("5000"));
    }

    // ─── Customer Operations ─────────────────────────────────────────

    #[tokio::test]
    async fn get_customer_parses_response() {
        let mock = MockStripeServer::start().await;
        mock.on(
            "/v1/customers/",
            json!({
                "id": "cus_test",
                "email": "test@example.com",
                "name": "Test User",
                "metadata": {"org_id": "org-123"}
            }),
        );

        let customer = mock.client().get_customer("cus_test").await.unwrap();
        assert_eq!(customer.id, "cus_test");
        assert_eq!(customer.email.as_deref(), Some("test@example.com"));
        assert_eq!(customer.metadata.get("org_id").unwrap(), "org-123");
    }

    #[tokio::test]
    async fn find_customer_by_org_returns_none_for_empty() {
        let mock = MockStripeServer::start().await;
        mock.on("/v1/customers/search", json!({"data": []}));

        let customer = mock
            .client()
            .find_customer_by_org("org-missing")
            .await
            .unwrap();
        assert!(customer.is_none());
    }

    #[tokio::test]
    async fn find_customer_by_org_skips_deleted() {
        let mock = MockStripeServer::start().await;
        mock.on(
            "/v1/customers/search",
            json!({
                "data": [{
                    "id": "cus_del",
                    "email": "del@example.com",
                    "deleted": true,
                    "metadata": {"org_id": "org-1"}
                }]
            }),
        );

        let customer = mock.client().find_customer_by_org("org-1").await.unwrap();
        assert!(customer.is_none());
    }

    #[tokio::test]
    async fn create_customer_sends_metadata() {
        let mock = MockStripeServer::start().await;
        mock.on(
            "/v1/customers",
            json!({
                "id": "cus_new",
                "email": "new@example.com",
                "metadata": {"org_id": "org-new"}
            }),
        );

        let customer = mock
            .client()
            .create_customer("new@example.com", "New User", "org-new")
            .await
            .unwrap();
        assert_eq!(customer.id, "cus_new");
    }

    // ─── Subscriptions ───────────────────────────────────────────────

    #[tokio::test]
    async fn get_subscription_parses_items() {
        let mock = MockStripeServer::start().await;
        mock.on(
            "/v1/subscriptions/",
            json!({
                "id": "sub_test",
                "customer": "cus_abc",
                "status": "active",
                "cancel_at_period_end": false,
                "items": {
                    "data": [{
                        "id": "si_1",
                        "price": {
                            "id": "price_starter",
                            "product": "prod_starter",
                            "unit_amount": 2900
                        }
                    }]
                },
                "metadata": {"org_id": "org-1"}
            }),
        );

        let sub = mock.client().get_subscription("sub_test").await.unwrap();
        assert_eq!(sub.id, "sub_test");
        assert_eq!(sub.customer_id(), Some("cus_abc".to_string()));
        assert_eq!(sub.product_id(), Some("prod_starter".to_string()));
        assert!(!sub.is_free_plan());
    }

    #[test]
    fn subscription_free_plan_detection() {
        let sub: crate::stripe::client::StripeSubscription = serde_json::from_value(json!({
            "id": "sub_free",
            "customer": "cus_abc",
            "status": "active",
            "items": {"data": [{"id": "si_1", "price": {"id": "p", "product": "prod", "unit_amount": 0}}]},
            "metadata": {}
        }))
        .unwrap();
        assert!(sub.is_free_plan());
    }

    // ─── Products ────────────────────────────────────────────────────

    #[tokio::test]
    async fn get_product_parses_metadata() {
        let mock = MockStripeServer::start().await;
        mock.on(
            "/v1/products/",
            json!({
                "id": "prod_test",
                "name": "Starter",
                "metadata": {"tier": "starter"}
            }),
        );

        let product = mock.client().get_product("prod_test").await.unwrap();
        assert_eq!(product.name, "Starter");
        assert_eq!(product.metadata.get("tier").unwrap(), "starter");
    }

    // ─── Invoices ────────────────────────────────────────────────────

    #[tokio::test]
    async fn create_invoice_item_sends_amount() {
        let mock = MockStripeServer::start().await;
        mock.on("/v1/invoiceitems", json!({"id": "ii_test"}));

        mock.client()
            .create_invoice_item("cus_abc", 5000, "Auto top-up", &[("type", "auto_topup")])
            .await
            .unwrap();

        let reqs = mock.recorded();
        let req = reqs
            .iter()
            .find(|(_, path, _)| path.contains("invoiceitems"))
            .unwrap();
        assert!(req.2.contains("5000"));
    }

    #[tokio::test]
    async fn create_and_finalize_invoice() {
        let mock = MockStripeServer::start().await;
        mock.on(
            "/v1/invoices",
            json!({"id": "inv_123", "customer": "cus_abc", "metadata": {}}),
        );
        // The finalize endpoint path includes the invoice ID
        mock.on(
            "finalize",
            json!({"id": "inv_123", "customer": "cus_abc", "metadata": {}}),
        );

        let invoice_id = mock
            .client()
            .create_and_finalize_invoice("cus_abc", &[("type", "auto_topup")])
            .await
            .unwrap();

        assert_eq!(invoice_id, "inv_123");
    }

    // ─── Meter Events ────────────────────────────────────────────────

    #[tokio::test]
    async fn send_meter_event_skips_zero() {
        let mock = MockStripeServer::start().await;

        mock.client()
            .send_meter_event("llm_spend", "cus_abc", 0, 1234567890)
            .await
            .unwrap();

        assert!(mock.recorded().is_empty());
    }

    #[tokio::test]
    async fn send_meter_event_sends_params() {
        let mock = MockStripeServer::start().await;
        mock.on("/v1/billing/meter_events", json!({"id": "me_test"}));

        mock.client()
            .send_meter_event("llm_spend", "cus_abc", 42, 1234567890)
            .await
            .unwrap();

        let reqs = mock.recorded();
        assert_eq!(reqs.len(), 1);
        assert!(reqs[0].2.contains("llm_spend"));
        assert!(reqs[0].2.contains("42"));
    }

    // ─── Webhook Signature Verification ──────────────────────────────

    #[test]
    fn verify_webhook_roundtrip() {
        // Use the client's own signing logic via a known test vector
        let secret = "whsec_test_secret";
        let payload = r#"{"id":"evt_1","type":"invoice.paid","data":{"object":{}}}"#;

        // Sign it
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let sig = sign_payload(secret, ts, payload);
        let header = format!("t={ts},v1={sig}");

        let event = StripeClient::verify_webhook(payload.as_bytes(), &header, secret).unwrap();
        assert_eq!(event.event_type, "invoice.paid");
    }

    #[test]
    fn verify_webhook_rejects_bad_signature() {
        let result =
            StripeClient::verify_webhook(b"test", "t=9999999999,v1=badsignature", "whsec_test");
        assert!(result.is_err());
    }

    #[test]
    fn verify_webhook_rejects_missing_v1() {
        let result = StripeClient::verify_webhook(b"test", "t=1234567890", "whsec_test");
        assert!(result.is_err());
    }

    #[test]
    fn verify_webhook_rejects_old_timestamp() {
        let secret = "whsec_test";
        let payload = r#"{"id":"evt_old","type":"test","data":{"object":{}}}"#;
        let old_ts = 1577836800u64; // 2020-01-01
        let sig = sign_payload(secret, old_ts, payload);
        let header = format!("t={old_ts},v1={sig}");

        let result = StripeClient::verify_webhook(payload.as_bytes(), &header, secret);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("timestamp too old"),
        );
    }

    // ─── Serde: New Response Types ──────────────────────────────────

    #[test]
    fn deser_checkout_session_embedded() {
        let session: crate::stripe::client::StripeCheckoutSession = serde_json::from_value(json!({
            "id": "cs_test_abc",
            "mode": "subscription",
            "payment_status": "paid",
            "customer": "cus_123",
            "amount_total": 2900,
            "metadata": {"org_id": "org-1"},
            "client_secret": "cs_test_abc_secret_xyz"
        }))
        .unwrap();
        assert_eq!(session.id, "cs_test_abc");
        assert_eq!(session.mode.as_deref(), Some("subscription"));
        assert_eq!(session.customer_id(), Some("cus_123".to_string()));
        assert_eq!(session.amount_total, Some(2900));
        assert_eq!(session.metadata.get("org_id").unwrap(), "org-1");
    }

    #[test]
    fn deser_checkout_session_customer_as_object() {
        let session: crate::stripe::client::StripeCheckoutSession = serde_json::from_value(json!({
            "id": "cs_test_def",
            "customer": {"id": "cus_456", "email": "test@example.com"},
            "metadata": {}
        }))
        .unwrap();
        assert_eq!(session.customer_id(), Some("cus_456".to_string()));
    }

    #[test]
    fn deser_checkout_session_no_customer() {
        let session: crate::stripe::client::StripeCheckoutSession = serde_json::from_value(json!({
            "id": "cs_test_ghi",
            "metadata": {}
        }))
        .unwrap();
        assert_eq!(session.customer_id(), None);
    }

    #[test]
    fn deser_checkout_session_redirect() {
        let session: crate::stripe::client::StripeCheckoutSessionRedirect =
            serde_json::from_value(json!({
                "id": "cs_test_redir",
                "url": "https://checkout.stripe.com/pay/cs_test_redir"
            }))
            .unwrap();
        assert_eq!(session.id, "cs_test_redir");
        assert_eq!(
            session.url.as_deref(),
            Some("https://checkout.stripe.com/pay/cs_test_redir")
        );
    }

    #[test]
    fn deser_checkout_session_redirect_null_url() {
        let session: crate::stripe::client::StripeCheckoutSessionRedirect =
            serde_json::from_value(json!({
                "id": "cs_test_no_url",
                "url": null
            }))
            .unwrap();
        assert!(session.url.is_none());
    }

    #[test]
    fn deser_portal_session() {
        let session: crate::stripe::client::StripePortalSession = serde_json::from_value(json!({
            "id": "bps_test_123",
            "url": "https://billing.stripe.com/session/test_YWNj",
            "created": 1234567890,
            "customer": "cus_abc",
            "return_url": "https://example.com/billing"
        }))
        .unwrap();
        assert_eq!(session.id, "bps_test_123");
        assert_eq!(session.url, "https://billing.stripe.com/session/test_YWNj");
    }

    #[test]
    fn deser_payment_method_with_card() {
        let pm: crate::stripe::client::StripePaymentMethod = serde_json::from_value(json!({
            "id": "pm_test_visa",
            "type": "card",
            "card": {
                "brand": "visa",
                "last4": "4242",
                "exp_month": 12,
                "exp_year": 2027,
                "funding": "credit",
                "country": "US"
            },
            "customer": "cus_abc"
        }))
        .unwrap();
        assert_eq!(pm.id, "pm_test_visa");
        let card = pm.card.unwrap();
        assert_eq!(card.brand.as_deref(), Some("visa"));
        assert_eq!(card.last4.as_deref(), Some("4242"));
        assert_eq!(card.exp_month, Some(12));
        assert_eq!(card.exp_year, Some(2027));
    }

    #[test]
    fn deser_payment_method_no_card() {
        let pm: crate::stripe::client::StripePaymentMethod = serde_json::from_value(json!({
            "id": "pm_test_bank",
            "type": "us_bank_account"
        }))
        .unwrap();
        assert_eq!(pm.id, "pm_test_bank");
        assert!(pm.card.is_none());
        assert!(pm.customer.is_none());
    }

    #[test]
    fn deser_payment_method_customer_as_object() {
        let pm: crate::stripe::client::StripePaymentMethod = serde_json::from_value(json!({
            "id": "pm_test",
            "customer": {"id": "cus_789"}
        }))
        .unwrap();
        // customer is serde_json::Value, verify it round-trips
        assert_eq!(
            pm.customer.unwrap().as_object().unwrap().get("id").unwrap(),
            "cus_789"
        );
    }

    #[test]
    fn deser_list_of_payment_methods() {
        let list: crate::stripe::client::StripeList<crate::stripe::client::StripePaymentMethod> =
            serde_json::from_value(json!({
                "object": "list",
                "data": [
                    {"id": "pm_1", "card": {"brand": "visa", "last4": "4242", "exp_month": 1, "exp_year": 2026}},
                    {"id": "pm_2", "card": {"brand": "mastercard", "last4": "5555", "exp_month": 6, "exp_year": 2028}}
                ],
                "has_more": false,
                "url": "/v1/payment_methods"
            }))
            .unwrap();
        assert_eq!(list.data.len(), 2);
        assert_eq!(list.data[0].id, "pm_1");
        assert_eq!(
            list.data[1].card.as_ref().unwrap().brand.as_deref(),
            Some("mastercard")
        );
    }

    #[test]
    fn deser_list_empty() {
        let list: crate::stripe::client::StripeList<crate::stripe::client::StripePaymentMethod> =
            serde_json::from_value(json!({
                "object": "list",
                "data": [],
                "has_more": false
            }))
            .unwrap();
        assert!(list.data.is_empty());
    }

    #[test]
    fn deser_list_of_subscriptions() {
        let list: crate::stripe::client::StripeList<crate::stripe::client::StripeSubscription> =
            serde_json::from_value(json!({
                "data": [
                    {
                        "id": "sub_1",
                        "customer": "cus_abc",
                        "status": "active",
                        "cancel_at_period_end": false,
                        "items": {"data": [{"id": "si_1", "price": {"id": "price_1", "product": "prod_1", "unit_amount": 2900}}]},
                        "metadata": {"org_id": "org-1"}
                    },
                    {
                        "id": "sub_2",
                        "customer": "cus_abc",
                        "status": "canceled",
                        "items": {"data": []},
                        "metadata": {}
                    }
                ]
            }))
            .unwrap();
        assert_eq!(list.data.len(), 2);
        assert_eq!(list.data[0].status, "active");
        assert_eq!(list.data[1].status, "canceled");
    }

    #[test]
    fn deser_list_of_products() {
        let list: crate::stripe::client::StripeList<crate::stripe::client::StripeProduct> =
            serde_json::from_value(json!({
                "data": [
                    {"id": "prod_free", "name": "Free", "metadata": {"tier": "free"}},
                    {"id": "prod_pro", "name": "Pro", "metadata": {"tier": "pro"}}
                ]
            }))
            .unwrap();
        assert_eq!(list.data.len(), 2);
        assert_eq!(list.data[0].name, "Free");
        assert_eq!(list.data[1].metadata.get("tier").unwrap(), "pro");
    }

    #[test]
    fn deser_list_of_prices() {
        let list: crate::stripe::client::StripeList<crate::stripe::client::StripePrice> =
            serde_json::from_value(json!({
                "data": [
                    {"id": "price_free", "product": "prod_free", "unit_amount": 0},
                    {"id": "price_pro", "product": "prod_pro", "unit_amount": 2900},
                    {"id": "price_metered", "product": "prod_usage"}
                ]
            }))
            .unwrap();
        assert_eq!(list.data.len(), 3);
        assert_eq!(list.data[0].unit_amount, Some(0));
        assert_eq!(list.data[1].unit_amount, Some(2900));
        assert_eq!(list.data[2].unit_amount, None); // metered prices have no unit_amount
    }

    #[test]
    fn deser_subscription_with_expanded_price() {
        let sub: crate::stripe::client::StripeSubscription = serde_json::from_value(json!({
            "id": "sub_expanded",
            "customer": "cus_abc",
            "status": "active",
            "cancel_at_period_end": false,
            "cancel_at": 1700000000,
            "items": {
                "data": [{
                    "id": "si_1",
                    "price": {
                        "id": "price_pro",
                        "product": {"id": "prod_pro", "name": "Pro Plan", "metadata": {}},
                        "unit_amount": 4900
                    }
                }]
            },
            "metadata": {"org_id": "org-1"}
        }))
        .unwrap();
        assert_eq!(sub.cancel_at, Some(1700000000));
        // product_id works with expanded product object
        assert_eq!(sub.product_id(), Some("prod_pro".to_string()));
        assert_eq!(sub.price_id(), Some("price_pro".to_string()));
        assert!(!sub.is_free_plan());
    }

    #[test]
    fn deser_subscription_cancel_at_period_end() {
        let sub: crate::stripe::client::StripeSubscription = serde_json::from_value(json!({
            "id": "sub_cancelling",
            "customer": "cus_abc",
            "status": "active",
            "cancel_at_period_end": true,
            "cancel_at": 1700000000,
            "items": {"data": []},
            "metadata": {}
        }))
        .unwrap();
        assert!(sub.cancel_at_period_end);
        assert_eq!(sub.cancel_at, Some(1700000000));
    }

    #[test]
    fn deser_subscription_minimal() {
        // Stripe sometimes returns minimal fields
        let sub: crate::stripe::client::StripeSubscription = serde_json::from_value(json!({
            "id": "sub_min",
            "customer": "cus_abc",
            "status": "incomplete",
            "items": {"data": []},
            "metadata": {}
        }))
        .unwrap();
        assert!(!sub.cancel_at_period_end);
        assert!(sub.cancel_at.is_none());
        assert!(sub.ended_at.is_none());
        assert!(sub.start_date.is_none());
        assert!(sub.product_id().is_none());
        assert!(sub.price_id().is_none());
        assert!(sub.is_free_plan()); // no items = free
    }

    #[test]
    fn deser_invoice_with_metadata() {
        let inv: crate::stripe::client::StripeInvoice = serde_json::from_value(json!({
            "id": "inv_test",
            "customer": "cus_abc",
            "amount_paid": 5000,
            "amount_total": 5000,
            "metadata": {"type": "auto_topup", "org_id": "org-1", "credit_cents": "5000"}
        }))
        .unwrap();
        assert_eq!(inv.id, "inv_test");
        assert_eq!(inv.customer_id(), Some("cus_abc".to_string()));
        assert_eq!(inv.amount_paid, Some(5000));
        assert_eq!(inv.metadata.get("type").unwrap(), "auto_topup");
        assert_eq!(inv.metadata.get("credit_cents").unwrap(), "5000");
    }

    #[test]
    fn deser_invoice_customer_as_object() {
        let inv: crate::stripe::client::StripeInvoice = serde_json::from_value(json!({
            "id": "inv_obj",
            "customer": {"id": "cus_obj", "email": "a@b.com"},
            "metadata": {}
        }))
        .unwrap();
        assert_eq!(inv.customer_id(), Some("cus_obj".to_string()));
    }

    #[test]
    fn deser_stripe_event() {
        let event: crate::stripe::client::StripeEvent = serde_json::from_value(json!({
            "id": "evt_test_123",
            "type": "customer.subscription.created",
            "data": {
                "object": {
                    "id": "sub_abc",
                    "customer": "cus_xyz",
                    "status": "active"
                }
            }
        }))
        .unwrap();
        assert_eq!(event.id, "evt_test_123");
        assert_eq!(event.event_type, "customer.subscription.created");
        assert_eq!(event.data.object.get("id").unwrap(), "sub_abc");
    }

    #[test]
    fn deser_customer_with_deleted() {
        let c: crate::stripe::client::StripeCustomer = serde_json::from_value(json!({
            "id": "cus_del",
            "deleted": true
        }))
        .unwrap();
        assert_eq!(c.deleted, Some(true));
        assert!(c.email.is_none());
        assert!(c.name.is_none());
    }

    #[test]
    fn deser_customer_minimal() {
        let c: crate::stripe::client::StripeCustomer = serde_json::from_value(json!({
            "id": "cus_min"
        }))
        .unwrap();
        assert_eq!(c.id, "cus_min");
        assert!(c.deleted.is_none());
        assert!(c.metadata.is_empty());
    }

    // ─── HTTP Handler ────────────────────────────────────────────────

    // Tests for billing::http handler paths that don't touch DB/Stripe.

    #[tokio::test]
    async fn webhook_handler_missing_signature_header() {
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let state = crate::http::BillingHttpState {
            stripe: StripeClient::new("sk_test_fake").unwrap(),
            db: make_dummy_billing_db().await,
            webhook_secret: "whsec_test".to_string(),
        };
        let app = crate::http::billing_router(state);

        let req = Request::builder()
            .method("POST")
            .uri("/webhooks/stripe")
            .body(Body::from("{}"))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn webhook_handler_invalid_signature() {
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let state = crate::http::BillingHttpState {
            stripe: StripeClient::new("sk_test_fake").unwrap(),
            db: make_dummy_billing_db().await,
            webhook_secret: "whsec_test".to_string(),
        };
        let app = crate::http::billing_router(state);

        let req = Request::builder()
            .method("POST")
            .uri("/webhooks/stripe")
            .header("stripe-signature", "t=1234567890,v1=bad")
            .body(Body::from("{}"))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    /// Create a BillingDb that will fail on any real query, but satisfies
    /// the type system for testing handler paths that return early.
    async fn make_dummy_billing_db() -> crate::db::BillingDb {
        // Connect to a non-existent DB. The pool will be created but
        // any actual query will fail. This is fine because our tests
        // only exercise paths that return before touching the DB.
        crate::db::BillingDb::connect("postgres://invalid:invalid@localhost:1/nonexistent")
            .await
            .unwrap()
    }

    /// Sign a payload using the same HMAC-SHA256 algorithm as StripeClient.
    /// We compute it ourselves using ring (a dev-dependency) to verify the
    /// built-in implementation.
    fn sign_payload(secret: &str, timestamp: u64, payload: &str) -> String {
        use ring::hmac;
        let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
        let signed = format!("{timestamp}.{payload}");
        let tag = hmac::sign(&key, signed.as_bytes());
        tag.as_ref().iter().map(|b| format!("{b:02x}")).collect()
    }
}

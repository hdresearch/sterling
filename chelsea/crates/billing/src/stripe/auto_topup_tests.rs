//! Tests for auto-topup service.

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::stripe::auto_topup::check_and_topup;
    use crate::stripe::test_helpers::MockStripeServer;

    fn balance_response(amount: i64) -> serde_json::Value {
        json!({
            "balances": [{
                "available_balance": {"monetary": {"value": amount, "currency": "usd"}}
            }]
        })
    }

    fn setup_invoice_mocks(mock: &MockStripeServer) {
        mock.on("/v1/invoiceitems", json!({"id": "ii_test"}));
        mock.on(
            "/v1/invoices",
            json!({"id": "inv_topup", "customer": "cus_abc", "metadata": {}}),
        );
        mock.on(
            "finalize",
            json!({"id": "inv_topup", "customer": "cus_abc", "metadata": {}}),
        );
    }

    #[tokio::test]
    async fn triggers_when_below_threshold() {
        let mock = MockStripeServer::start().await;
        mock.on("/v1/billing/credit_balance_summary", balance_response(2000));
        setup_invoice_mocks(&mock);

        let result = check_and_topup(&mock.client(), "org-1", "cus_abc", 5000, 10000).await;

        assert!(result.triggered);
        assert!(result.error.is_none());

        // Should have called: balance + invoiceitem + invoice + finalize
        let reqs = mock.recorded();
        assert!(reqs.len() >= 3);
    }

    #[tokio::test]
    async fn not_triggered_above_threshold() {
        let mock = MockStripeServer::start().await;
        mock.on(
            "/v1/billing/credit_balance_summary",
            balance_response(10000),
        );

        let result = check_and_topup(&mock.client(), "org-1", "cus_abc", 5000, 10000).await;

        assert!(!result.triggered);
        assert!(result.error.is_none());
        assert_eq!(mock.recorded().len(), 1, "only balance check");
    }

    #[tokio::test]
    async fn not_triggered_at_exact_threshold() {
        let mock = MockStripeServer::start().await;
        mock.on("/v1/billing/credit_balance_summary", balance_response(5000));

        let result = check_and_topup(&mock.client(), "org-1", "cus_abc", 5000, 10000).await;
        assert!(!result.triggered);
    }

    #[tokio::test]
    async fn triggers_at_zero_balance() {
        let mock = MockStripeServer::start().await;
        mock.on(
            "/v1/billing/credit_balance_summary",
            json!({"balances": []}),
        );
        setup_invoice_mocks(&mock);

        let result = check_and_topup(&mock.client(), "org-1", "cus_abc", 100, 5000).await;
        assert!(result.triggered);
    }
}

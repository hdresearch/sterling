//! Low-level Stripe API client for billing meter events and credit balance queries.

use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;
use tracing::{debug, error, warn};

use crate::error::StripeError;

const STRIPE_API_BASE: &str = "https://api.stripe.com";

#[derive(Clone)]
pub struct StripeClient {
    http: Client,
    api_key: String,
    base_url: String,
}

impl StripeClient {
    pub fn new(api_key: impl Into<String>) -> Result<Self, reqwest::Error> {
        Self::new_with_base_url(api_key, STRIPE_API_BASE)
    }

    /// Create a client with a custom base URL (for testing with mock servers).
    pub fn new_with_base_url(
        api_key: impl Into<String>,
        base_url: &str,
    ) -> Result<Self, reqwest::Error> {
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self {
            http,
            api_key: api_key.into(),
            base_url: base_url.trim_end_matches('/').to_string(),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    // ─── Meter Events ────────────────────────────────────────────────

    /// Send a single meter event to Stripe.
    ///
    /// `event_name`: the billing meter name (e.g. "llm_spend")
    /// `customer_id`: Stripe customer ID (cus_xxx)
    /// `value`: integer usage units
    /// `timestamp`: Unix seconds
    pub async fn send_meter_event(
        &self,
        event_name: &str,
        customer_id: &str,
        value: i64,
        timestamp: i64,
    ) -> Result<(), StripeError> {
        if value <= 0 {
            return Ok(());
        }

        let form = [
            ("event_name", event_name.to_string()),
            ("payload[stripe_customer_id]", customer_id.to_string()),
            ("payload[value]", value.to_string()),
            ("timestamp", timestamp.to_string()),
        ];

        debug!(
            event_name,
            customer_id, value, timestamp, "sending Stripe meter event"
        );

        let resp = self
            .http
            .post(&self.url("/v1/billing/meter_events"))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .form(&form)
            .send()
            .await?;

        if resp.status().is_success() {
            debug!("stripe meter event accepted");
            return Ok(());
        }

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();

        // 4xx (except 429) are not retryable
        if status.is_client_error() && status.as_u16() != 429 {
            error!(%status, %body, "stripe meter event rejected (not retryable)");
            return Err(StripeError::Api { status, body });
        }

        warn!(%status, %body, "stripe meter event failed (retryable)");
        Err(StripeError::Api { status, body })
    }

    // ─── Credit Balance ──────────────────────────────────────────────

    /// Fetch the available credit balance for a Stripe customer.
    /// Returns the available amount in cents.
    pub async fn get_credit_balance(&self, customer_id: &str) -> Result<i64, StripeError> {
        let resp = self
            .http
            .get(&self.url("/v1/billing/credit_balance_summary"))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .query(&[
                ("customer", customer_id),
                ("filter[type]", "applicability_scope"),
                ("filter[applicability_scope][price_type]", "metered"),
            ])
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(StripeError::Api { status, body });
        }

        let summary: CreditBalanceSummary = resp.json().await?;

        let available: i64 = summary
            .balances
            .iter()
            .filter_map(|b| b.available_balance.as_ref())
            .filter_map(|ab| ab.monetary.as_ref())
            .map(|m| m.value)
            .sum();

        Ok(available)
    }

    // ─── Credit Grants ─────────────────────────────────────────────

    /// Create a credit grant for a customer (for top-ups and auto-topups).
    pub async fn create_credit_grant(
        &self,
        customer_id: &str,
        amount_cents: i64,
        category: &str,
        metadata: &[(&str, &str)],
    ) -> Result<(), StripeError> {
        let mut form = vec![
            ("customer".to_string(), customer_id.to_string()),
            ("category".to_string(), category.to_string()),
            ("amount[type]".to_string(), "monetary".to_string()),
            ("amount[monetary][currency]".to_string(), "usd".to_string()),
            (
                "amount[monetary][value]".to_string(),
                amount_cents.to_string(),
            ),
            (
                "applicability_config[scope][price_type]".to_string(),
                "metered".to_string(),
            ),
        ];
        for (k, v) in metadata {
            form.push((format!("metadata[{k}]"), v.to_string()));
        }

        let resp = self
            .http
            .post(&self.url("/v1/billing/credit_grants"))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .form(&form)
            .send()
            .await?;

        if resp.status().is_success() {
            debug!(customer_id, amount_cents, "stripe credit grant created");
            return Ok(());
        }

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        error!(%status, %body, "stripe credit grant failed");
        Err(StripeError::Api { status, body })
    }

    // ─── Customers ───────────────────────────────────────────────────

    /// Retrieve a Stripe customer by ID. Returns metadata and email.
    pub async fn get_customer(&self, customer_id: &str) -> Result<StripeCustomer, StripeError> {
        let resp = self
            .http
            .get(&format!("{}/v1/customers/{customer_id}", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(StripeError::Api { status, body });
        }

        Ok(resp.json().await?)
    }

    /// Search for a customer by metadata org_id.
    pub async fn find_customer_by_org(
        &self,
        org_id: &str,
    ) -> Result<Option<StripeCustomer>, StripeError> {
        let query = format!("metadata[\"org_id\"]:\"{org_id}\"");
        let resp = self
            .http
            .get(&self.url("/v1/customers/search"))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .query(&[("query", &query)])
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(StripeError::Api { status, body });
        }

        let list: StripeList<StripeCustomer> = resp.json().await?;
        Ok(list.data.into_iter().find(|c| !c.deleted.unwrap_or(false)))
    }

    /// Create a Stripe customer with org_id metadata.
    pub async fn create_customer(
        &self,
        email: &str,
        name: &str,
        org_id: &str,
    ) -> Result<StripeCustomer, StripeError> {
        let form = [
            ("email", email),
            ("name", name),
            ("metadata[org_id]", org_id),
        ];

        let resp = self
            .http
            .post(&self.url("/v1/customers"))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .form(&form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(StripeError::Api { status, body });
        }

        Ok(resp.json().await?)
    }

    // ─── Subscriptions ───────────────────────────────────────────────

    /// Retrieve a subscription by ID.
    pub async fn get_subscription(
        &self,
        subscription_id: &str,
    ) -> Result<StripeSubscription, StripeError> {
        let resp = self
            .http
            .get(&format!(
                "{}/v1/subscriptions/{subscription_id}",
                self.base_url
            ))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .query(&[("expand[]", "items.data.price")])
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(StripeError::Api { status, body });
        }

        Ok(resp.json().await?)
    }

    /// Retrieve a product by ID.
    pub async fn get_product(&self, product_id: &str) -> Result<StripeProduct, StripeError> {
        let resp = self
            .http
            .get(&format!("{}/v1/products/{product_id}", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(StripeError::Api { status, body });
        }

        Ok(resp.json().await?)
    }

    // ─── Invoices (for auto-topup) ──────────────────────────────────

    /// Create an invoice item (pending charge) on a customer.
    pub async fn create_invoice_item(
        &self,
        customer_id: &str,
        amount_cents: i64,
        description: &str,
        metadata: &[(&str, &str)],
    ) -> Result<(), StripeError> {
        let mut form = vec![
            ("customer".to_string(), customer_id.to_string()),
            ("amount".to_string(), amount_cents.to_string()),
            ("currency".to_string(), "usd".to_string()),
            ("description".to_string(), description.to_string()),
        ];
        for (k, v) in metadata {
            form.push((format!("metadata[{k}]"), v.to_string()));
        }

        let resp = self
            .http
            .post(&self.url("/v1/invoiceitems"))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .form(&form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(StripeError::Api { status, body });
        }
        Ok(())
    }

    /// Create and finalize an invoice to trigger auto-charge.
    pub async fn create_and_finalize_invoice(
        &self,
        customer_id: &str,
        metadata: &[(&str, &str)],
    ) -> Result<String, StripeError> {
        // Create invoice
        let mut form = vec![
            ("customer".to_string(), customer_id.to_string()),
            ("auto_advance".to_string(), "true".to_string()),
            (
                "collection_method".to_string(),
                "charge_automatically".to_string(),
            ),
        ];
        for (k, v) in metadata {
            form.push((format!("metadata[{k}]"), v.to_string()));
        }

        let resp = self
            .http
            .post(&self.url("/v1/invoices"))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .form(&form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(StripeError::Api { status, body });
        }

        let invoice: StripeInvoice = resp.json().await?;
        let invoice_id = invoice.id.clone();

        // Finalize to trigger payment
        let resp = self
            .http
            .post(&format!(
                "{}/v1/invoices/{invoice_id}/finalize",
                self.base_url
            ))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!(%status, %body, "invoice finalize failed");
            return Err(StripeError::Api { status, body });
        }

        debug!(invoice_id, customer_id, "invoice created and finalized");
        Ok(invoice_id)
    }

    // ─── Subscription Lifecycle ─────────────────────────────────────

    /// List subscriptions for a customer.
    pub async fn list_subscriptions(
        &self,
        customer_id: &str,
        status: Option<&str>,
        limit: Option<u32>,
    ) -> Result<StripeList<StripeSubscription>, StripeError> {
        let mut query = vec![("customer", customer_id.to_string())];
        if let Some(s) = status {
            query.push(("status", s.to_string()));
        }
        query.push(("limit", limit.unwrap_or(10).to_string()));
        query.push(("expand[]", "data.items.data.price".to_string()));

        let resp = self
            .http
            .get(&self.url("/v1/subscriptions"))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .query(&query)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(StripeError::Api { status, body });
        }

        Ok(resp.json().await?)
    }

    /// Create a subscription for a customer.
    pub async fn create_subscription(
        &self,
        customer_id: &str,
        price_id: &str,
        metadata: &[(&str, &str)],
    ) -> Result<StripeSubscription, StripeError> {
        let mut form = vec![
            ("customer".to_string(), customer_id.to_string()),
            ("items[0][price]".to_string(), price_id.to_string()),
        ];
        for (k, v) in metadata {
            form.push((format!("metadata[{k}]"), v.to_string()));
        }

        let resp = self
            .http
            .post(&self.url("/v1/subscriptions"))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .form(&form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(StripeError::Api { status, body });
        }

        Ok(resp.json().await?)
    }

    /// Update a subscription item's price (for plan changes).
    pub async fn update_subscription(
        &self,
        subscription_id: &str,
        item_id: &str,
        new_price_id: &str,
        proration: &str,
    ) -> Result<StripeSubscription, StripeError> {
        let form = [
            ("items[0][id]", item_id),
            ("items[0][price]", new_price_id),
            ("proration_behavior", proration),
        ];

        let resp = self
            .http
            .post(&format!(
                "{}/v1/subscriptions/{subscription_id}",
                self.base_url
            ))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .form(&form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(StripeError::Api { status, body });
        }

        Ok(resp.json().await?)
    }

    /// Cancel a subscription immediately.
    pub async fn cancel_subscription(
        &self,
        subscription_id: &str,
    ) -> Result<StripeSubscription, StripeError> {
        let resp = self
            .http
            .delete(&format!(
                "{}/v1/subscriptions/{subscription_id}",
                self.base_url
            ))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(StripeError::Api { status, body });
        }

        Ok(resp.json().await?)
    }

    /// Schedule cancellation at period end (or undo it).
    pub async fn set_cancel_at_period_end(
        &self,
        subscription_id: &str,
        cancel: bool,
    ) -> Result<StripeSubscription, StripeError> {
        let form = [(
            "cancel_at_period_end",
            if cancel { "true" } else { "false" },
        )];

        let resp = self
            .http
            .post(&format!(
                "{}/v1/subscriptions/{subscription_id}",
                self.base_url
            ))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .form(&form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(StripeError::Api { status, body });
        }

        Ok(resp.json().await?)
    }

    // ─── Checkout & Portal Sessions ──────────────────────────────────

    /// Create a checkout session (for subscriptions or one-time payments).
    pub async fn create_checkout_session(
        &self,
        customer_id: &str,
        price_id: &str,
        mode: &str,
        return_url: &str,
        metadata: &[(&str, &str)],
    ) -> Result<StripeCheckoutSession, StripeError> {
        let mut form = vec![
            ("customer".to_string(), customer_id.to_string()),
            ("mode".to_string(), mode.to_string()),
            ("ui_mode".to_string(), "embedded".to_string()),
            ("line_items[0][price]".to_string(), price_id.to_string()),
            ("line_items[0][quantity]".to_string(), "1".to_string()),
            ("return_url".to_string(), return_url.to_string()),
        ];
        for (k, v) in metadata {
            form.push((format!("metadata[{k}]"), v.to_string()));
        }

        let resp = self
            .http
            .post(&self.url("/v1/checkout/sessions"))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .form(&form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(StripeError::Api { status, body });
        }

        Ok(resp.json().await?)
    }

    /// Create a top-up checkout session with inline price data.
    pub async fn create_topup_checkout_session(
        &self,
        customer_id: &str,
        amount_cents: i64,
        success_url: &str,
        cancel_url: &str,
        metadata: &[(&str, &str)],
    ) -> Result<StripeCheckoutSessionRedirect, StripeError> {
        let mut form = vec![
            ("customer".to_string(), customer_id.to_string()),
            ("mode".to_string(), "payment".to_string()),
            (
                "line_items[0][price_data][currency]".to_string(),
                "usd".to_string(),
            ),
            (
                "line_items[0][price_data][unit_amount]".to_string(),
                amount_cents.to_string(),
            ),
            (
                "line_items[0][price_data][product_data][name]".to_string(),
                format!("Account Credits — ${:.2}", amount_cents as f64 / 100.0),
            ),
            (
                "line_items[0][price_data][product_data][description]".to_string(),
                "Prepaid credits applied to metered usage".to_string(),
            ),
            ("line_items[0][quantity]".to_string(), "1".to_string()),
            ("success_url".to_string(), success_url.to_string()),
            ("cancel_url".to_string(), cancel_url.to_string()),
        ];
        for (k, v) in metadata {
            form.push((format!("metadata[{k}]"), v.to_string()));
        }

        let resp = self
            .http
            .post(&self.url("/v1/checkout/sessions"))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .form(&form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(StripeError::Api { status, body });
        }

        Ok(resp.json().await?)
    }

    /// Create a billing portal session.
    pub async fn create_portal_session(
        &self,
        customer_id: &str,
        return_url: &str,
    ) -> Result<StripePortalSession, StripeError> {
        let form = [("customer", customer_id), ("return_url", return_url)];

        let resp = self
            .http
            .post(&self.url("/v1/billing_portal/sessions"))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .form(&form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(StripeError::Api { status, body });
        }

        Ok(resp.json().await?)
    }

    /// Create a setup session (for adding payment methods without charging).
    pub async fn create_setup_session(
        &self,
        customer_id: &str,
        success_url: &str,
        cancel_url: &str,
        metadata: &[(&str, &str)],
    ) -> Result<StripeCheckoutSessionRedirect, StripeError> {
        let mut form = vec![
            ("customer".to_string(), customer_id.to_string()),
            ("mode".to_string(), "setup".to_string()),
            ("currency".to_string(), "usd".to_string()),
            ("success_url".to_string(), success_url.to_string()),
            ("cancel_url".to_string(), cancel_url.to_string()),
        ];
        for (k, v) in metadata {
            form.push((format!("metadata[{k}]"), v.to_string()));
        }

        let resp = self
            .http
            .post(&self.url("/v1/checkout/sessions"))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .form(&form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(StripeError::Api { status, body });
        }

        Ok(resp.json().await?)
    }

    // ─── Payment Methods ─────────────────────────────────────────────

    /// List payment methods for a customer.
    pub async fn list_payment_methods(
        &self,
        customer_id: &str,
    ) -> Result<StripeList<StripePaymentMethod>, StripeError> {
        let resp = self
            .http
            .get(&self.url("/v1/payment_methods"))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .query(&[("customer", customer_id), ("type", "card")])
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(StripeError::Api { status, body });
        }

        Ok(resp.json().await?)
    }

    /// Detach a payment method from a customer.
    pub async fn detach_payment_method(&self, payment_method_id: &str) -> Result<(), StripeError> {
        let resp = self
            .http
            .post(&format!(
                "{}/v1/payment_methods/{payment_method_id}/detach",
                self.base_url
            ))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(StripeError::Api { status, body });
        }

        Ok(())
    }

    // ─── Product Catalog ─────────────────────────────────────────────

    /// List active products.
    pub async fn list_products(&self) -> Result<StripeList<StripeProduct>, StripeError> {
        let resp = self
            .http
            .get(&self.url("/v1/products"))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .query(&[("active", "true"), ("limit", "100")])
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(StripeError::Api { status, body });
        }

        Ok(resp.json().await?)
    }

    /// List active prices.
    pub async fn list_prices(&self) -> Result<StripeList<StripePrice>, StripeError> {
        let resp = self
            .http
            .get(&self.url("/v1/prices"))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .query(&[("active", "true"), ("limit", "100")])
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(StripeError::Api { status, body });
        }

        Ok(resp.json().await?)
    }

    // ─── Webhook Signature Verification ──────────────────────────────

    /// Verify a Stripe webhook signature and parse the event.
    ///
    /// `payload`: raw request body bytes
    /// `sig_header`: value of the `Stripe-Signature` header
    /// `secret`: webhook endpoint secret (whsec_xxx)
    pub fn verify_webhook(
        payload: &[u8],
        sig_header: &str,
        secret: &str,
    ) -> Result<StripeEvent, StripeError> {
        // Parse the signature header
        let mut timestamp: Option<&str> = None;
        let mut signatures: Vec<&str> = Vec::new();

        for part in sig_header.split(',') {
            let part = part.trim();
            if let Some(t) = part.strip_prefix("t=") {
                timestamp = Some(t);
            } else if let Some(v1) = part.strip_prefix("v1=") {
                signatures.push(v1);
            }
        }

        let ts = timestamp.ok_or_else(|| StripeError::Api {
            status: reqwest::StatusCode::BAD_REQUEST,
            body: "missing timestamp in signature".into(),
        })?;

        if signatures.is_empty() {
            return Err(StripeError::Api {
                status: reqwest::StatusCode::BAD_REQUEST,
                body: "no v1 signatures found".into(),
            });
        }

        // Compute expected signature
        let signed_payload = format!("{ts}.{}", std::str::from_utf8(payload).unwrap_or(""));
        let expected = hmac_sha256(secret.as_bytes(), signed_payload.as_bytes());

        // Constant-time compare
        let matched = signatures
            .iter()
            .any(|sig| constant_time_eq(&expected, sig));
        if !matched {
            return Err(StripeError::Api {
                status: reqwest::StatusCode::BAD_REQUEST,
                body: "webhook signature verification failed".into(),
            });
        }

        // Verify timestamp is recent (within 5 minutes)
        if let Ok(ts_secs) = ts.parse::<i64>() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            if (now - ts_secs).abs() > 300 {
                return Err(StripeError::Api {
                    status: reqwest::StatusCode::BAD_REQUEST,
                    body: "webhook timestamp too old".into(),
                });
            }
        }

        // Parse the event
        serde_json::from_slice(payload).map_err(|e| StripeError::Api {
            status: reqwest::StatusCode::BAD_REQUEST,
            body: format!("invalid event JSON: {e}"),
        })
    }
}

// ─── HMAC-SHA256 via ring ────────────────────────────────────────────────────

fn hmac_sha256(key: &[u8], data: &[u8]) -> String {
    let k = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key);
    let tag = ring::hmac::sign(&k, data);
    tag.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

// ─── Stripe API response types ───────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct CreditBalanceSummary {
    #[serde(default)]
    balances: Vec<CreditBalance>,
}

#[derive(Debug, Deserialize)]
struct CreditBalance {
    available_balance: Option<BalanceAmount>,
}

#[derive(Debug, Deserialize)]
struct BalanceAmount {
    monetary: Option<MonetaryAmount>,
}

#[derive(Debug, Deserialize)]
struct MonetaryAmount {
    value: i64,
}

// ─── Public Stripe types ─────────────────────────────────────────────────────

/// Stripe Event (webhook payload).
#[derive(Debug, Clone, Deserialize)]
pub struct StripeEvent {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub data: StripeEventData,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StripeEventData {
    pub object: serde_json::Value,
}

/// Stripe Customer.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct StripeCustomer {
    pub id: String,
    pub email: Option<String>,
    pub name: Option<String>,
    #[serde(default)]
    pub metadata: std::collections::HashMap<String, String>,
    pub deleted: Option<bool>,
}

/// Stripe Subscription.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct StripeSubscription {
    pub id: String,
    pub customer: serde_json::Value, // can be string or object
    pub status: String,
    #[serde(default)]
    pub cancel_at_period_end: bool,
    pub cancel_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub start_date: Option<i64>,
    #[serde(default)]
    pub items: StripeSubscriptionItems,
    #[serde(default)]
    pub metadata: std::collections::HashMap<String, String>,
}

impl StripeSubscription {
    /// Extract the customer ID (handles both string and object forms).
    pub fn customer_id(&self) -> Option<String> {
        self.customer.as_str().map(|s| s.to_string()).or_else(|| {
            self.customer
                .as_object()
                .and_then(|o| o.get("id"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
    }

    /// Get the first line item's product ID.
    pub fn product_id(&self) -> Option<String> {
        self.items.data.first().and_then(|item| {
            item.price.as_ref().and_then(|p| match &p.product {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Object(o) => {
                    o.get("id").and_then(|v| v.as_str()).map(|s| s.to_string())
                }
                _ => None,
            })
        })
    }

    /// Get the first line item's price ID.
    pub fn price_id(&self) -> Option<String> {
        self.items
            .data
            .first()
            .and_then(|item| item.price.as_ref().map(|p| p.id.clone()))
    }

    /// Check if this is a free plan (unit_amount == 0).
    pub fn is_free_plan(&self) -> bool {
        self.items
            .data
            .first()
            .and_then(|item| item.price.as_ref())
            .map(|p| p.unit_amount.unwrap_or(0) == 0)
            .unwrap_or(true)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct StripeSubscriptionItems {
    #[serde(default)]
    pub data: Vec<StripeSubscriptionItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StripeSubscriptionItem {
    pub id: String,
    pub price: Option<StripePrice>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct StripePrice {
    pub id: String,
    pub product: serde_json::Value, // string or Product object
    pub unit_amount: Option<i64>,
}

/// Stripe Product.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct StripeProduct {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub metadata: std::collections::HashMap<String, String>,
}

/// Stripe Invoice.
#[derive(Debug, Clone, Deserialize)]
pub struct StripeInvoice {
    pub id: String,
    pub customer: serde_json::Value,
    pub amount_paid: Option<i64>,
    pub amount_total: Option<i64>,
    #[serde(default)]
    pub metadata: std::collections::HashMap<String, String>,
}

impl StripeInvoice {
    pub fn customer_id(&self) -> Option<String> {
        self.customer.as_str().map(|s| s.to_string()).or_else(|| {
            self.customer
                .as_object()
                .and_then(|o| o.get("id"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
    }
}

/// Stripe Checkout Session.
#[derive(Debug, Clone, Deserialize)]
pub struct StripeCheckoutSession {
    pub id: String,
    pub mode: Option<String>,
    pub payment_status: Option<String>,
    pub customer: Option<serde_json::Value>,
    pub amount_total: Option<i64>,
    #[serde(default)]
    pub metadata: std::collections::HashMap<String, String>,
}

impl StripeCheckoutSession {
    pub fn customer_id(&self) -> Option<String> {
        self.customer.as_ref().and_then(|c| {
            c.as_str().map(|s| s.to_string()).or_else(|| {
                c.as_object()
                    .and_then(|o| o.get("id"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
        })
    }
}

/// Stripe Checkout Session with redirect URL (non-embedded mode).
#[derive(Debug, Clone, Deserialize)]
pub struct StripeCheckoutSessionRedirect {
    pub id: String,
    pub url: Option<String>,
}

/// Stripe Billing Portal session.
#[derive(Debug, Clone, Deserialize)]
pub struct StripePortalSession {
    pub id: String,
    pub url: String,
}

/// Stripe Payment Method.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct StripePaymentMethod {
    pub id: String,
    pub card: Option<StripeCard>,
    pub customer: Option<serde_json::Value>,
}

/// Stripe Card details.
#[derive(Debug, Clone, Deserialize)]
pub struct StripeCard {
    pub brand: Option<String>,
    pub last4: Option<String>,
    pub exp_month: Option<u32>,
    pub exp_year: Option<u32>,
}

/// Stripe list wrapper.
#[derive(Debug, Deserialize)]
pub struct StripeList<T> {
    #[serde(default)]
    pub data: Vec<T>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_webhook_valid_signature() {
        let secret = "whsec_test_secret";
        let payload = r#"{"id":"evt_test","type":"test","data":{"object":{}}}"#;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let signed = format!("{timestamp}.{payload}");
        let sig = hmac_sha256(secret.as_bytes(), signed.as_bytes());
        let header = format!("t={timestamp},v1={sig}");

        let result = StripeClient::verify_webhook(payload.as_bytes(), &header, secret);
        assert!(result.is_ok());
        let event = result.unwrap();
        assert_eq!(event.event_type, "test");
    }

    #[test]
    fn verify_webhook_bad_signature() {
        let payload = r#"{"id":"evt_test","type":"test","data":{"object":{}}}"#;
        let header = "t=1234567890,v1=badsig";
        let result = StripeClient::verify_webhook(payload.as_bytes(), header, "whsec_test");
        assert!(result.is_err());
    }

    #[test]
    fn subscription_customer_id_from_string() {
        let sub: StripeSubscription = serde_json::from_str(
            r#"{"id":"sub_1","customer":"cus_123","status":"active","items":{"data":[]},"metadata":{}}"#,
        ).unwrap();
        assert_eq!(sub.customer_id(), Some("cus_123".to_string()));
    }

    #[test]
    fn subscription_customer_id_from_object() {
        let sub: StripeSubscription = serde_json::from_str(
            r#"{"id":"sub_1","customer":{"id":"cus_456"},"status":"active","items":{"data":[]},"metadata":{}}"#,
        ).unwrap();
        assert_eq!(sub.customer_id(), Some("cus_456".to_string()));
    }
}

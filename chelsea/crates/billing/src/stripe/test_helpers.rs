//! Test helpers: mock Stripe HTTP server and event builders.

#![cfg(test)]

use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::client::StripeClient;

/// A mock Stripe API server that records requests and returns configured responses.
pub struct MockStripeServer {
    pub addr: String,
    /// Recorded requests: Vec<(method, path, body)>
    pub requests: Arc<Mutex<Vec<(String, String, String)>>>,
    /// Response overrides: path_prefix → response body JSON
    responses: Arc<Mutex<HashMap<String, Value>>>,
    _handle: tokio::task::JoinHandle<()>,
}

impl MockStripeServer {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let addr = format!("http://127.0.0.1:{port}");
        let requests: Arc<Mutex<Vec<(String, String, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let responses: Arc<Mutex<HashMap<String, Value>>> = Arc::new(Mutex::new(HashMap::new()));

        let req_clone = requests.clone();
        let resp_clone = responses.clone();

        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let reqs = req_clone.clone();
                let resps = resp_clone.clone();

                tokio::spawn(async move {
                    let mut buf = vec![0u8; 65536];
                    let n = stream.read(&mut buf).await.unwrap_or(0);
                    let raw = String::from_utf8_lossy(&buf[..n]).to_string();

                    // Parse HTTP request line
                    let first_line = raw.lines().next().unwrap_or("");
                    let parts: Vec<&str> = first_line.split_whitespace().collect();
                    let method = parts.first().copied().unwrap_or("GET").to_string();
                    let path = parts.get(1).copied().unwrap_or("/").to_string();

                    // Extract body (after \r\n\r\n)
                    let body = raw.split("\r\n\r\n").nth(1).unwrap_or("").to_string();

                    reqs.lock().unwrap().push((method, path.clone(), body));

                    // Find matching response (longest match wins)
                    let response_body = {
                        let map = resps.lock().unwrap();
                        let mut best: Option<(usize, serde_json::Value)> = None;
                        for (prefix, val) in map.iter() {
                            if path.starts_with(prefix) || path.contains(prefix) {
                                let score = prefix.len();
                                if best.as_ref().map_or(true, |(s, _)| score > *s) {
                                    best = Some((score, val.clone()));
                                }
                            }
                        }
                        best.map(|(_, v)| v)
                            .unwrap_or(json!({"id": "mock_default"}))
                    };

                    let body_str = serde_json::to_string(&response_body).unwrap();
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                        body_str.len(),
                        body_str
                    );
                    let _ = stream.write_all(resp.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });

        Self {
            addr,
            requests,
            responses,
            _handle: handle,
        }
    }

    /// Set a response for requests matching a path prefix.
    pub fn on(&self, path_prefix: &str, response: Value) {
        self.responses
            .lock()
            .unwrap()
            .insert(path_prefix.to_string(), response);
    }

    /// Get all recorded requests.
    pub fn recorded(&self) -> Vec<(String, String, String)> {
        self.requests.lock().unwrap().clone()
    }

    /// Create a StripeClient pointing at this mock server.
    pub fn client(&self) -> StripeClient {
        StripeClient::new_with_base_url("sk_test_mock", &self.addr).unwrap()
    }
}

// ─── Event builders ──────────────────────────────────────────────────────────

/// Build a StripeEvent for testing.
pub fn make_event(event_type: &str, object: Value) -> super::client::StripeEvent {
    super::client::StripeEvent {
        id: format!("evt_test_{}", uuid::Uuid::new_v4()),
        event_type: event_type.to_string(),
        data: super::client::StripeEventData { object },
    }
}

/// Build a subscription JSON object.
pub fn subscription_json(
    sub_id: &str,
    customer_id: &str,
    status: &str,
    org_id: Option<&str>,
) -> Value {
    let mut metadata = json!({});
    if let Some(oid) = org_id {
        metadata = json!({"org_id": oid});
    }
    json!({
        "id": sub_id,
        "customer": customer_id,
        "status": status,
        "cancel_at_period_end": false,
        "items": {
            "data": [{
                "id": "si_test",
                "price": {
                    "id": "price_starter",
                    "product": "prod_starter",
                    "unit_amount": 2900
                }
            }]
        },
        "metadata": metadata
    })
}

/// Build a checkout session JSON object.
pub fn checkout_session_json(
    session_id: &str,
    customer_id: &str,
    org_id: &str,
    amount: i64,
) -> Value {
    json!({
        "id": session_id,
        "mode": "payment",
        "payment_status": "paid",
        "customer": customer_id,
        "amount_total": amount,
        "metadata": {
            "org_id": org_id
        }
    })
}

/// Build an invoice JSON object.
pub fn invoice_json(
    invoice_id: &str,
    customer_id: &str,
    org_id: &str,
    amount: i64,
    is_auto_topup: bool,
) -> Value {
    let mut metadata = json!({"org_id": org_id});
    if is_auto_topup {
        metadata = json!({
            "org_id": org_id,
            "type": "auto_topup",
            "credit_cents": amount.to_string()
        });
    }
    json!({
        "id": invoice_id,
        "customer": customer_id,
        "amount_paid": amount,
        "metadata": metadata
    })
}

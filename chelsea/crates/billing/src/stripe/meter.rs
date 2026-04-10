//! Batched meter event sender.
//!
//! Request handlers send `(stripe_customer_id, cost_millicents)` to an mpsc channel.
//! A background task accumulates per-customer and flushes to Stripe periodically.
//!
//! Unit convention: 1 meter unit = 1 millicent ($0.001). The Stripe metered price
//! must be configured at $0.001/unit so that 1000 units = $1.00 drawn from credits.

use rust_decimal::Decimal;
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use super::client::StripeClient;

/// A usage event to be metered.
pub struct UsageEvent {
    pub stripe_customer_id: String,
    /// Cost in dollars (Decimal), as returned by spend::calculate_cost.
    pub cost: Decimal,
}

/// Handle for submitting meter events from request handlers.
#[derive(Clone)]
pub struct MeterEventSender {
    tx: mpsc::UnboundedSender<UsageEvent>,
}

impl MeterEventSender {
    /// Submit a usage event for batching. Non-blocking, never fails
    /// (silently drops if the receiver is gone).
    pub fn send(&self, event: UsageEvent) {
        if self.tx.send(event).is_err() {
            warn!("meter event channel closed, dropping event");
        }
    }
}

/// Spawn the background meter event flusher.
///
/// Returns a `MeterEventSender` handle that request handlers use to submit events.
///
/// `flush_interval`: how often to flush accumulated events to Stripe.
/// `meter_event_name`: the Stripe billing meter name (e.g. "llm_spend").
pub fn spawn_meter_task(
    client: StripeClient,
    meter_event_name: String,
    flush_interval: Duration,
) -> MeterEventSender {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(meter_flush_loop(
        client,
        meter_event_name,
        rx,
        flush_interval,
    ));
    MeterEventSender { tx }
}

/// Millicent conversion: cost in dollars → integer millicents.
/// 1 millicent = $0.001. Rounds up so we never under-bill.
fn cost_to_millicents(cost: Decimal) -> i64 {
    use rust_decimal::prelude::ToPrimitive;
    let millicents = cost * Decimal::from(1000);
    // ceil to avoid under-billing on fractional millicents
    millicents.ceil().to_i64().unwrap_or(0)
}

async fn meter_flush_loop(
    client: StripeClient,
    meter_event_name: String,
    mut rx: mpsc::UnboundedReceiver<UsageEvent>,
    flush_interval: Duration,
) {
    info!(
        flush_interval_ms = flush_interval.as_millis(),
        meter = %meter_event_name,
        "meter event flusher started"
    );

    // Accumulator: stripe_customer_id → accumulated millicents
    let mut accum: HashMap<String, i64> = HashMap::new();

    loop {
        // Drain channel until flush interval elapses or channel closes
        let deadline = tokio::time::sleep(flush_interval);
        tokio::pin!(deadline);

        loop {
            tokio::select! {
                biased;

                event = rx.recv() => {
                    match event {
                        Some(e) => {
                            let mc = cost_to_millicents(e.cost);
                            if mc > 0 {
                                *accum.entry(e.stripe_customer_id).or_default() += mc;
                            }
                        }
                        None => {
                            // Channel closed — flush remaining and exit
                            info!("meter event channel closed, flushing remaining events");
                            flush(&client, &meter_event_name, &mut accum).await;
                            return;
                        }
                    }
                }
                _ = &mut deadline => {
                    break;
                }
            }
        }

        flush(&client, &meter_event_name, &mut accum).await;
    }
}

async fn flush(client: &StripeClient, meter_event_name: &str, accum: &mut HashMap<String, i64>) {
    if accum.is_empty() {
        return;
    }

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let batch: Vec<(String, i64)> = accum.drain().collect();
    let count = batch.len();
    let total_mc: i64 = batch.iter().map(|(_, v)| v).sum();

    info!(
        customers = count,
        total_millicents = total_mc,
        "flushing meter events to Stripe"
    );

    for (customer_id, value) in batch {
        // Retry once on transient failure
        for attempt in 1..=2u32 {
            match client
                .send_meter_event(meter_event_name, &customer_id, value, timestamp)
                .await
            {
                Ok(()) => break,
                Err(e) => {
                    if attempt == 1 {
                        warn!(
                            customer_id = %customer_id,
                            value,
                            error = %e,
                            "meter event failed, retrying"
                        );
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    } else {
                        error!(
                            customer_id = %customer_id,
                            value,
                            error = %e,
                            "meter event failed after retry — usage lost"
                        );
                        // TODO: write to a dead letter table for manual reconciliation
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn cost_to_millicents_rounds_up() {
        // $0.75 = 750 millicents
        assert_eq!(cost_to_millicents(dec!(0.75)), 750);
        // $0.001 = 1 millicent
        assert_eq!(cost_to_millicents(dec!(0.001)), 1);
        // $0.0001 = 0.1 millicent → rounds up to 1
        assert_eq!(cost_to_millicents(dec!(0.0001)), 1);
        // $0.00075 = 0.75 millicent → rounds up to 1
        assert_eq!(cost_to_millicents(dec!(0.00075)), 1);
        // $12.50 = 12500 millicents
        assert_eq!(cost_to_millicents(dec!(12.50)), 12500);
        // $0 = 0
        assert_eq!(cost_to_millicents(dec!(0)), 0);
    }
}

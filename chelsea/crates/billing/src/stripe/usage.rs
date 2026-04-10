//! VM usage metering — reports CPU, storage, and bandwidth usage to Stripe.
//!
//! Usage is aggregated and reported at the **monthly** level. Stripe sums
//! all meter events over the billing period and invoices once at period end.
//!
//! Meter event units:
//!   - `cpu`: vCPU-seconds (price: $0.001667/unit = $0.06/vCPU-hour)
//!   - `storage`: GB-months (price: $0.08/unit)
//!   - `bandwidth`: GB transferred (price: $0.09/unit)

use tracing::{debug, error, info};

use super::client::StripeClient;
use crate::error::StripeError;

/// A usage record for one customer in one reporting interval.
#[derive(Debug, Clone)]
pub struct UsageRecord {
    /// Stripe customer ID (cus_xxx).
    pub stripe_customer_id: String,
    /// vCPU-seconds consumed in this interval.
    pub cpu_vcpu_seconds: i64,
    /// Storage in GB-months for this interval.
    pub storage_gb_months: i64,
    /// Bandwidth in GB transferred in this interval.
    pub bandwidth_gb: i64,
    /// Unix timestamp for the meter event.
    pub timestamp: i64,
}

/// Meter event names matching the Stripe billing meters.
pub const METER_CPU: &str = "cpu";
pub const METER_STORAGE: &str = "storage";
pub const METER_BANDWIDTH: &str = "bandwidth";

/// Report a usage record to Stripe as individual meter events.
///
/// Sends one event per non-zero metric. Returns the count of events sent.
pub async fn report_usage(client: &StripeClient, record: &UsageRecord) -> Result<u32, StripeError> {
    let mut sent = 0u32;

    if record.cpu_vcpu_seconds > 0 {
        client
            .send_meter_event(
                METER_CPU,
                &record.stripe_customer_id,
                record.cpu_vcpu_seconds,
                record.timestamp,
            )
            .await?;
        debug!(
            customer = %record.stripe_customer_id,
            vcpu_seconds = record.cpu_vcpu_seconds,
            "reported CPU usage"
        );
        sent += 1;
    }

    if record.storage_gb_months > 0 {
        client
            .send_meter_event(
                METER_STORAGE,
                &record.stripe_customer_id,
                record.storage_gb_months,
                record.timestamp,
            )
            .await?;
        debug!(
            customer = %record.stripe_customer_id,
            gb_months = record.storage_gb_months,
            "reported storage usage"
        );
        sent += 1;
    }

    if record.bandwidth_gb > 0 {
        client
            .send_meter_event(
                METER_BANDWIDTH,
                &record.stripe_customer_id,
                record.bandwidth_gb,
                record.timestamp,
            )
            .await?;
        debug!(
            customer = %record.stripe_customer_id,
            gb = record.bandwidth_gb,
            "reported bandwidth usage"
        );
        sent += 1;
    }

    if sent > 0 {
        info!(
            customer = %record.stripe_customer_id,
            events = sent,
            cpu_vcpu_seconds = record.cpu_vcpu_seconds,
            storage_gb_months = record.storage_gb_months,
            bandwidth_gb = record.bandwidth_gb,
            "usage reported to Stripe"
        );
    }

    Ok(sent)
}

/// Report a batch of usage records. Returns (events_sent, customers_errored).
pub async fn report_usage_batch(client: &StripeClient, records: &[UsageRecord]) -> (u32, u32) {
    let mut total_sent = 0u32;
    let mut total_errors = 0u32;

    for record in records {
        match report_usage(client, record).await {
            Ok(sent) => total_sent += sent,
            Err(e) => {
                error!(
                    customer = %record.stripe_customer_id,
                    error = %e,
                    "failed to report usage to Stripe"
                );
                total_errors += 1;
            }
        }
    }

    (total_sent, total_errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_names_are_correct() {
        assert_eq!(METER_CPU, "cpu");
        assert_eq!(METER_STORAGE, "storage");
        assert_eq!(METER_BANDWIDTH, "bandwidth");
    }
}

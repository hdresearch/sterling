//! Resolves VM usage records to Stripe customers and reports meter events.

use std::collections::{HashMap, HashSet};

use thiserror::Error;
use tracing::{error, info, warn};
use uuid::Uuid;

use billing::stripe::{
    client::StripeClient,
    usage::{UsageRecord, report_usage_batch},
};

use crate::db::{ApiKeyEntity, ApiKeysRepository, DB, DBError, VMsRepository};

/// Stripe context for usage forwarding.
#[derive(Clone)]
pub struct StripeUsageContext {
    pub client: StripeClient,
    pub billing_db: billing::db::BillingDb,
}

#[derive(Debug, Clone)]
pub struct ForwardUsageRecord {
    pub vm_id: Uuid,
    pub owner_api_key_id: Option<Uuid>,
    pub recorded_hour: i64,
    pub cpu_usage: i64,
    pub storage_usage: i64,
    pub vm_node_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct BatchSummary {
    pub node_id: String,
    pub interval_start: i64,
    pub interval_end: i64,
    pub record_count: usize,
    pub event_count: usize,
}

#[derive(Debug, Error)]
pub enum UsageForwardError {
    #[error("stripe usage forwarding is disabled")]
    Disabled,
    #[error("vm '{0}' not found while processing usage")]
    VmNotFound(String),
    #[error("api key '{0}' not found while processing usage")]
    ApiKeyNotFound(Uuid),
    #[error("org '{0}' not found while processing usage")]
    OrgNotFound(Uuid),
    #[error("database error: {0}")]
    Db(#[from] DBError),
    #[error("billing db error: {0}")]
    BillingDb(#[from] billing::error::DbError),
}

/// Resolves owner context, aggregates usage per Stripe customer, and sends meter events.
pub async fn forward_usage_records(
    db: DB,
    stripe: &Option<StripeUsageContext>,
    records: Vec<ForwardUsageRecord>,
    interval_start: i64,
    interval_end: i64,
    batch_node_id: &str,
) -> Result<BatchSummary, UsageForwardError> {
    let stripe = stripe.as_ref().ok_or(UsageForwardError::Disabled)?;

    let record_count = records.len();

    if records.is_empty() {
        return Ok(BatchSummary {
            node_id: batch_node_id.to_string(),
            interval_start,
            interval_end,
            record_count: 0,
            event_count: 0,
        });
    }

    // Caches to avoid repeated DB lookups within the same batch.
    let mut vm_owner_cache: HashMap<Uuid, Uuid> = HashMap::new();
    let mut api_key_cache: HashMap<Uuid, ApiKeyEntity> = HashMap::new();
    let mut org_customer_cache: HashMap<Uuid, Option<String>> = HashMap::new();
    let mut skipped_orgs: HashSet<Uuid> = HashSet::new();

    // Aggregate usage per Stripe customer.
    let mut customer_usage: HashMap<String, (i64, i64)> = HashMap::new(); // (cpu_seconds, storage_seconds)

    for record in &records {
        // Resolve owner API key ID
        let owner_id = if let Some(owner) = record.owner_api_key_id {
            owner
        } else if let Some(owner) = vm_owner_cache.get(&record.vm_id) {
            *owner
        } else {
            let vm = db
                .vms()
                .get_by_id(record.vm_id)
                .await?
                .ok_or_else(|| UsageForwardError::VmNotFound(record.vm_id.to_string()))?;
            let owner = vm.owner_id();
            vm_owner_cache.insert(record.vm_id, owner);
            owner
        };

        // Resolve API key → org
        let api_key = if let Some(key) = api_key_cache.get(&owner_id) {
            key.clone()
        } else {
            let key = db
                .keys()
                .get_by_id(owner_id)
                .await?
                .ok_or(UsageForwardError::ApiKeyNotFound(owner_id))?;
            api_key_cache.insert(owner_id, key.clone());
            key
        };

        let org_id = api_key.org_id();
        if skipped_orgs.contains(&org_id) {
            continue;
        }

        // Resolve org → Stripe customer ID
        let stripe_customer_id = if let Some(cached) = org_customer_cache.get(&org_id) {
            cached.clone()
        } else {
            let customer_id = stripe
                .billing_db
                .get_stripe_customer_for_org(org_id)
                .await?;
            org_customer_cache.insert(org_id, customer_id.clone());
            customer_id
        };

        let stripe_customer_id = match stripe_customer_id {
            Some(id) => id,
            None => {
                if !skipped_orgs.contains(&org_id) {
                    warn!(
                        org_id = %org_id,
                        "no Stripe customer found for org; skipping usage"
                    );
                    skipped_orgs.insert(org_id);
                }
                continue;
            }
        };

        let entry = customer_usage.entry(stripe_customer_id).or_default();
        entry.0 += record.cpu_usage;
        entry.1 += record.storage_usage;
    }

    // Build usage records and send to Stripe.
    let usage_records: Vec<UsageRecord> = customer_usage
        .into_iter()
        .map(|(customer_id, (cpu, _storage))| UsageRecord {
            stripe_customer_id: customer_id,
            cpu_vcpu_seconds: cpu,
            storage_gb_months: 0, // TODO: convert storage_seconds to GB-months
            bandwidth_gb: 0,      // TODO: bandwidth tracking not yet implemented
            timestamp: interval_start,
        })
        .collect();

    let (sent, errors) = report_usage_batch(&stripe.client, &usage_records).await;

    if errors > 0 {
        error!(sent, errors, "some Stripe usage meter events failed");
    }

    info!(
        node_id = batch_node_id,
        interval_start,
        interval_end,
        records = record_count,
        stripe_events = sent,
        stripe_errors = errors,
        "usage batch forwarded to Stripe"
    );

    Ok(BatchSummary {
        node_id: batch_node_id.to_string(),
        interval_start,
        interval_end,
        record_count,
        event_count: sent as usize,
    })
}

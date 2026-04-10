use std::{
    collections::HashMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tokio::{
    sync::oneshot::{self, Receiver},
    time,
};
use tracing::{error, info, warn};
use uuid::Uuid;
use vers_config::VersConfig;

use crate::{
    db::{DB, DBError, OrchestratorEntity, UsageVmDescriptor, VmEvent},
    tokio_util::TokioTaskGracefulShutdown,
};

pub mod forwarder;

use forwarder::{ForwardUsageRecord, StripeUsageContext, UsageForwardError, forward_usage_records};

/// Periodically computes per-owner VM usage and forwards it to Stripe.
pub struct UsageReporter {
    db: DB,
    orchestrator_id: Uuid,
    stripe: Option<StripeUsageContext>,
}

#[derive(Debug, Clone)]
pub struct VmUsageRecord {
    pub vm_id: Uuid,
    pub owner_api_key_id: Uuid,
    /// `None` while the VM is sleeping.
    pub node_id: Option<Uuid>,
    pub recorded_hour: i64,
    pub cpu_usage: i64,
    pub mem_usage: i64,
    pub storage_usage: i64,
}

#[derive(thiserror::Error, Debug)]
pub enum UsageError {
    #[error("db error: {0}")]
    Db(#[from] DBError),
    #[error("system time error: {0}")]
    SystemTime(#[from] std::time::SystemTimeError),
    #[error("usage forwarding error: {0}")]
    Forward(UsageForwardError),
}

impl UsageReporter {
    pub fn new(db: DB, orchestrator_id: Uuid, stripe: Option<StripeUsageContext>) -> Self {
        Self {
            db,
            orchestrator_id,
            stripe,
        }
    }

    pub async fn run(self, mut shutdown: Receiver<()>) {
        info!("usage reporter enabled");

        if let Err(err) = self.catchup_missed_intervals().await {
            error!(?err, "usage reporter catch-up failed");
        }

        loop {
            let wait = match self.wait_duration() {
                Ok(duration) => duration,
                Err(err) => {
                    error!(?err, "failed to determine wait duration for usage reporter");
                    break;
                }
            };
            tokio::select! {
                _ = &mut shutdown => {
                    info!("usage reporter shutting down");
                    break;
                }
                _ = time::sleep(wait) => {}
            }

            if let Err(err) = self.process_latest_interval().await {
                error!(?err, "usage reporter failed to process interval");
            }
        }
    }

    async fn catchup_missed_intervals(&self) -> Result<(), UsageError> {
        let now = unix_timestamp()?;
        let current_hour_start = (now / 3600) * 3600;
        let last_interval = self
            .db
            .usage()
            .get_last_reported_interval(&self.orchestrator_id)
            .await?;

        let mut next_interval_start = match last_interval {
            Some((_, last_end)) => last_end,
            None => current_hour_start,
        };

        while next_interval_start + 3600 <= current_hour_start {
            let interval_end = next_interval_start + 3600;
            self.process_interval(next_interval_start, interval_end)
                .await?;
            next_interval_start = interval_end;
        }

        Ok(())
    }

    async fn process_latest_interval(&self) -> Result<(), UsageError> {
        let now = unix_timestamp()?;
        let current_hour_start = (now / 3600) * 3600;
        let interval_end = current_hour_start;
        let interval_start = interval_end - 3600;

        self.process_interval(interval_start, interval_end).await
    }

    async fn process_interval(
        &self,
        interval_start: i64,
        interval_end: i64,
    ) -> Result<(), UsageError> {
        let batch = self
            .calculate_usage_for_interval(interval_start, interval_end)
            .await?;

        self.deliver_batch(&batch).await?;

        self.db
            .usage()
            .update_last_reported_interval(&self.orchestrator_id, interval_start, interval_end)
            .await?;

        Ok(())
    }

    async fn calculate_usage_for_interval(
        &self,
        interval_start: i64,
        interval_end: i64,
    ) -> Result<UsageBatch, UsageError> {
        let vms = self
            .db
            .usage()
            .get_vms_with_usage_in_interval(interval_start, interval_end)
            .await?;

        let mut groups: HashMap<Uuid, Vec<VmUsageRecord>> = HashMap::new();

        for vm in vms {
            match self
                .calculate_vm_usage(&vm, interval_start, interval_end)
                .await
            {
                Ok(record) => {
                    groups
                        .entry(record.owner_api_key_id)
                        .or_default()
                        .push(record);
                }
                Err(err) => {
                    error!(
                        vm_id = %vm.vm_id,
                        ?err,
                        "failed to calculate usage for vm"
                    );
                }
            }
        }

        Ok(UsageBatch {
            groups,
            interval_start,
            interval_end,
        })
    }

    async fn calculate_vm_usage(
        &self,
        vm: &UsageVmDescriptor,
        interval_start: i64,
        interval_end: i64,
    ) -> Result<VmUsageRecord, UsageError> {
        let last_event_before = self
            .db
            .usage()
            .get_last_vm_event_before(&vm.vm_id, interval_start)
            .await?;
        let events = self
            .db
            .usage()
            .get_vm_events(&vm.vm_id, interval_start, interval_end)
            .await?;

        let usage = process_vm_events(&events, last_event_before, interval_start, interval_end, vm);

        Ok(usage)
    }

    async fn deliver_batch(&self, batch: &UsageBatch) -> Result<(), UsageError> {
        let vm_count: usize = batch.groups.values().map(|v| v.len()).sum();

        if vm_count == 0 {
            info!(
                "no usage data for interval {}-{}",
                batch.interval_start, batch.interval_end
            );
            return Ok(());
        }

        let mut forward_records = Vec::with_capacity(vm_count);
        for (owner, records) in &batch.groups {
            info!(
                owner_api_key_id = %owner,
                record_count = records.len(),
                interval_start = batch.interval_start,
                interval_end = batch.interval_end,
                records = ?records,
                "usage records grouped by api key"
            );
            for record in records {
                forward_records.push(ForwardUsageRecord {
                    vm_id: record.vm_id,
                    owner_api_key_id: Some(*owner),
                    recorded_hour: record.recorded_hour,
                    cpu_usage: record.cpu_usage,
                    storage_usage: record.storage_usage,
                    vm_node_id: record.node_id,
                });
            }
        }

        match forward_usage_records(
            self.db.clone(),
            &self.stripe,
            forward_records,
            batch.interval_start,
            batch.interval_end,
            &self.orchestrator_id.to_string(),
        )
        .await
        {
            Ok(summary) => {
                info!(
                    node_id = %summary.node_id,
                    interval_start = summary.interval_start,
                    interval_end = summary.interval_end,
                    records = summary.record_count,
                    stripe_events = summary.event_count,
                    "forwarded usage batch to Stripe"
                );
            }
            Err(UsageForwardError::Disabled) => {
                warn!(
                    "Stripe usage forwarding disabled; dropping batch for interval {}-{}",
                    batch.interval_start, batch.interval_end
                );
            }
            Err(err) => return Err(UsageError::Forward(err)),
        }

        Ok(())
    }

    fn wait_duration(&self) -> Result<Duration, UsageError> {
        if let Some(test_interval) = VersConfig::orchestrator().usage_reporting_test_interval_secs {
            return Ok(Duration::from_secs(test_interval));
        }

        let now = unix_timestamp()?;
        let current_hour_start = (now / 3600) * 3600;
        let next_hour_start = current_hour_start + 3600;
        let target_time = next_hour_start + 60;
        let wait_secs = if target_time > now {
            (target_time - now) as u64
        } else {
            0
        };
        Ok(Duration::from_secs(wait_secs))
    }
}

/// Reconstructs VM runtime within the interval and returns computed CPU/memory seconds.
fn process_vm_events(
    events: &[VmEvent],
    last_event_before: Option<VmEvent>,
    interval_start: i64,
    interval_end: i64,
    vm: &UsageVmDescriptor,
) -> VmUsageRecord {
    let mut total_cpu_seconds: i64 = 0;
    let mut total_mem_seconds: i64 = 0;

    let mut all_events = Vec::new();
    if let Some(event) = last_event_before {
        if matches!(event, VmEvent::Start(_)) {
            all_events.push(event);
        }
    }
    all_events.extend_from_slice(events);

    let mut i = 0;
    let mut last_was_start: Option<bool> = None;
    while i < all_events.len() {
        let current = &all_events[i];
        let is_start = matches!(current, VmEvent::Start(_));
        if let Some(prev) = last_was_start {
            if prev == is_start {
                let event_label = if is_start { "start" } else { "stop" };
                error!(
                    vm_id = %vm.vm_id,
                    interval_start,
                    interval_end,
                    event_index = i,
                    event_type = event_label,
                    "detected consecutive {event_label} events; skipping vm usage for interval"
                );
                return VmUsageRecord {
                    vm_id: vm.vm_id,
                    owner_api_key_id: vm.owner_id,
                    node_id: vm.node_id,
                    recorded_hour: interval_start,
                    cpu_usage: 0,
                    mem_usage: 0,
                    storage_usage: 0,
                };
            }
        }
        last_was_start = Some(is_start);

        if let VmEvent::Start(start_event) = current {
            let mut end_time = interval_end;
            for future in all_events.iter().skip(i + 1) {
                match future {
                    VmEvent::Stop(stop) => {
                        end_time = stop.timestamp;
                        break;
                    }
                    VmEvent::Start(next_start) => {
                        end_time = next_start.timestamp;
                        break;
                    }
                }
            }

            let duration_seconds = clamped_duration_seconds(
                start_event.timestamp,
                end_time,
                interval_start,
                interval_end,
            );

            if duration_seconds > 0 {
                total_cpu_seconds += (start_event.vcpu_count as i64) * duration_seconds;
                total_mem_seconds += (start_event.ram_mib as i64) * duration_seconds;
            }
        }
        i += 1;
    }

    VmUsageRecord {
        vm_id: vm.vm_id,
        owner_api_key_id: vm.owner_id,
        node_id: vm.node_id,
        recorded_hour: interval_start,
        cpu_usage: total_cpu_seconds,
        mem_usage: total_mem_seconds,
        storage_usage: 0,
    }
}

/// Returns the number of seconds between `start_time` and `end_time` that overlap the interval.
fn clamped_duration_seconds(
    start_time: i64,
    end_time: i64,
    interval_start: i64,
    interval_end: i64,
) -> i64 {
    let actual_start = start_time.max(interval_start);
    let actual_end = end_time.min(interval_end);

    if actual_end <= actual_start {
        return 0;
    }

    actual_end - actual_start
}

fn unix_timestamp() -> Result<i64, std::time::SystemTimeError> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64)
}

pub fn spawn_usage_task(
    db: DB,
    orch: OrchestratorEntity,
    stripe: Option<StripeUsageContext>,
) -> Option<TokioTaskGracefulShutdown> {
    if !VersConfig::orchestrator().usage_reporting_enabled {
        return None;
    }

    let (sender, receiver) = oneshot::channel();
    let reporter = UsageReporter::new(db, orch.id().clone(), stripe);
    let task = tokio::spawn(async move {
        reporter.run(receiver).await;
    });

    Some(TokioTaskGracefulShutdown {
        sender,
        task,
        label: Some("usage"),
    })
}

#[cfg(test)]
mod tests {
    use super::{clamped_duration_seconds, process_vm_events};
    use crate::db::{UsageVmDescriptor, VmEvent, VmStartEvent, VmStopEvent};
    use uuid::Uuid;

    #[test]
    fn clamps_duration_within_interval() {
        let duration = clamped_duration_seconds(0, 7200, 3600, 7200);
        assert_eq!(duration, 3600);
    }

    #[test]
    fn duration_is_zero_when_no_overlap() {
        let duration = clamped_duration_seconds(0, 1000, 2000, 3000);
        assert_eq!(duration, 0);
    }

    #[test]
    fn process_vm_events_counts_seconds_between_explicit_start_and_stop() {
        let vm = usage_vm();
        let events = vec![
            start_event(0, 2, 1_024),
            VmEvent::Stop(VmStopEvent { timestamp: 1_800 }),
        ];

        let record = process_vm_events(&events, None, 0, 3_600, &vm);

        assert_eq!(record.cpu_usage, 2 * 1_800);
        assert_eq!(record.mem_usage, 1_024 * 1_800);
        assert_eq!(record.recorded_hour, 0);
    }

    #[test]
    fn process_vm_events_handles_vm_running_entire_interval_from_prior_start() {
        let vm = usage_vm();
        let prior_start = VmEvent::Start(VmStartEvent {
            timestamp: -1_200,
            vcpu_count: 4,
            ram_mib: 2_048,
        });
        let events = Vec::new();

        let record = process_vm_events(&events, Some(prior_start), 0, 3_600, &vm);

        assert_eq!(record.cpu_usage, 4 * 3_600);
        assert_eq!(record.mem_usage, 2_048 * 3_600);
    }

    #[test]
    fn process_vm_events_skips_consecutive_start_events() {
        let vm = usage_vm();
        let events = vec![start_event(0, 2, 512), start_event(1_200, 4, 512)];

        let record = process_vm_events(&events, None, 0, 3_600, &vm);

        assert_eq!(record.cpu_usage, 0);
        assert_eq!(record.mem_usage, 0);
    }

    #[test]
    fn process_vm_events_handles_start_with_stop_inside_interval() {
        let vm = usage_vm();
        let prior_start = VmEvent::Start(VmStartEvent {
            timestamp: -300,
            vcpu_count: 6,
            ram_mib: 1_024,
        });
        let events = vec![VmEvent::Stop(VmStopEvent { timestamp: 900 })];

        let record = process_vm_events(&events, Some(prior_start), 0, 3_600, &vm);

        assert_eq!(record.cpu_usage, 6 * 900);
        assert_eq!(record.mem_usage, 1_024 * 900);
    }

    #[test]
    fn process_vm_events_handles_multiple_start_stop_pairs_within_interval() {
        let vm = usage_vm();
        let events = vec![
            start_event(0, 2, 256),
            VmEvent::Stop(VmStopEvent { timestamp: 600 }),
            start_event(1_200, 4, 512),
            VmEvent::Stop(VmStopEvent { timestamp: 1_800 }),
        ];

        let record = process_vm_events(&events, None, 0, 3_600, &vm);

        assert_eq!(record.cpu_usage, (2 * 600) + (4 * 600));
        assert_eq!(record.mem_usage, (256 * 600) + (512 * 600));
    }

    #[test]
    fn process_vm_events_handles_start_stop_start_without_final_stop() {
        let vm = usage_vm();
        let events = vec![
            start_event(0, 2, 256),
            VmEvent::Stop(VmStopEvent { timestamp: 600 }),
            start_event(1_800, 4, 512),
        ];

        let record = process_vm_events(&events, None, 0, 3_600, &vm);

        assert_eq!(record.cpu_usage, (2 * 600) + (4 * 1_800));
        assert_eq!(record.mem_usage, (256 * 600) + (512 * 1_800));
    }

    #[test]
    fn process_vm_events_skips_consecutive_stop_events() {
        let vm = usage_vm();
        let prior_start = VmEvent::Start(VmStartEvent {
            timestamp: -600,
            vcpu_count: 4,
            ram_mib: 1_024,
        });
        let events = vec![
            VmEvent::Stop(VmStopEvent { timestamp: 300 }),
            VmEvent::Stop(VmStopEvent { timestamp: 600 }),
        ];

        let record = process_vm_events(&events, Some(prior_start), 0, 3_600, &vm);

        assert_eq!(record.cpu_usage, 0);
        assert_eq!(record.mem_usage, 0);
    }

    #[test]
    fn process_vm_events_ignores_vms_that_finished_before_interval() {
        let vm = usage_vm();
        let last_event_before = VmEvent::Stop(VmStopEvent { timestamp: -10 });
        let events = Vec::new();

        let record = process_vm_events(&events, Some(last_event_before), 0, 3_600, &vm);

        assert_eq!(record.cpu_usage, 0);
        assert_eq!(record.mem_usage, 0);
    }

    #[test]
    fn process_vm_events_handles_missing_terminal_stop() {
        let vm = usage_vm();
        let events = vec![start_event(600, 8, 1_024)];

        let record = process_vm_events(&events, None, 0, 3_600, &vm);

        assert_eq!(record.cpu_usage, 8 * 3_000);
        assert_eq!(record.mem_usage, 1_024 * 3_000);
    }

    fn usage_vm() -> UsageVmDescriptor {
        UsageVmDescriptor {
            vm_id: Uuid::new_v4(),
            owner_id: Uuid::new_v4(),
            node_id: Some(Uuid::new_v4()),
        }
    }

    fn start_event(timestamp: i64, vcpu_count: u32, ram_mib: u32) -> VmEvent {
        VmEvent::Start(VmStartEvent {
            timestamp,
            vcpu_count,
            ram_mib,
        })
    }
}
/// Usage grouped by owner for a single reporting interval.
#[derive(Debug, Clone)]
pub struct UsageBatch {
    pub interval_start: i64,
    pub interval_end: i64,
    pub groups: HashMap<Uuid, Vec<VmUsageRecord>>,
}

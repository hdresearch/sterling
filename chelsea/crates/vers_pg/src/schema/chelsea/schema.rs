use std::sync::Arc;

use tokio_postgres::Client;

use crate::schema::chelsea::tables::{
    commit::TableCommit, sleep_snapshot::TableSleepSnapshot, vm::TableVm,
    vm_event_union::UnionVmEvent, vm_usage_segment::TableVmUsageSegment,
};

/// Schema `chelsea`
pub struct SchemaChelsea {
    pub commit: TableCommit,
    pub sleep_snapshot: TableSleepSnapshot,
    pub vm: TableVm,
    pub vm_usage_segment: TableVmUsageSegment,
    pub vm_event_union: UnionVmEvent,
}

impl SchemaChelsea {
    pub async fn new(client: Arc<Client>) -> Result<Self, crate::Error> {
        let commit = TableCommit::new(client.clone()).await?;
        let sleep_snapshot = TableSleepSnapshot::new(client.clone()).await?;
        let vm = TableVm::new(client.clone()).await?;
        let vm_usage_segment = TableVmUsageSegment::new(client.clone()).await?;
        let vm_event_union = UnionVmEvent::new(client.clone()).await?;

        Ok(Self {
            commit,
            sleep_snapshot,
            vm,
            vm_usage_segment,
            vm_event_union,
        })
    }
}

use serde::{Deserialize, Serialize};
use vers_pg::schema::chelsea::tables::sleep_snapshot::RecordVolumeSleepSnapshot;

use crate::volume_manager::ceph::CephVmVolumeSleepSnapshotMetadata;

/// Contains information about the volume to be stored on the VmSleepSnapshotMetadata
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum VmVolumeSleepSnapshotMetadata {
    Ceph(CephVmVolumeSleepSnapshotMetadata),
}

impl TryFrom<RecordVolumeSleepSnapshot> for VmVolumeSleepSnapshotMetadata {
    type Error = String;
    fn try_from(record: RecordVolumeSleepSnapshot) -> Result<Self, Self::Error> {
        match record {
            RecordVolumeSleepSnapshot::Ceph(record) => Ok(VmVolumeSleepSnapshotMetadata::Ceph(
                CephVmVolumeSleepSnapshotMetadata::try_from(record)?,
            )),
        }
    }
}

impl Into<RecordVolumeSleepSnapshot> for VmVolumeSleepSnapshotMetadata {
    fn into(self) -> RecordVolumeSleepSnapshot {
        match self {
            VmVolumeSleepSnapshotMetadata::Ceph(metadata) => {
                RecordVolumeSleepSnapshot::Ceph(metadata.into())
            }
        }
    }
}

use serde::{Deserialize, Serialize};
use vers_pg::schema::chelsea::tables::commit::RecordVolumeCommit;

use crate::volume_manager::ceph::CephVmVolumeCommitMetadata;

/// Contains information about the volume to be stored on the VmCommitMetadata
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum VmVolumeCommitMetadata {
    Ceph(CephVmVolumeCommitMetadata),
}

impl TryFrom<RecordVolumeCommit> for VmVolumeCommitMetadata {
    type Error = String;
    fn try_from(record: RecordVolumeCommit) -> Result<Self, Self::Error> {
        match record {
            RecordVolumeCommit::Ceph(record) => Ok(VmVolumeCommitMetadata::Ceph(
                CephVmVolumeCommitMetadata::try_from(record)?,
            )),
        }
    }
}

impl Into<RecordVolumeCommit> for VmVolumeCommitMetadata {
    fn into(self) -> RecordVolumeCommit {
        match self {
            VmVolumeCommitMetadata::Ceph(metadata) => RecordVolumeCommit::Ceph(metadata.into()),
        }
    }
}

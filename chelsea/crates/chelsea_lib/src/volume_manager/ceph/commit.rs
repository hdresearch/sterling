use ceph::RbdSnapName;
use serde::{Deserialize, Serialize};
use vers_pg::schema::chelsea::tables::commit::RecordCephVolumeCommit;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CephVmVolumeCommitMetadata {
    pub snap_name: RbdSnapName,
}

impl TryFrom<RecordCephVolumeCommit> for CephVmVolumeCommitMetadata {
    type Error = String;
    fn try_from(record: RecordCephVolumeCommit) -> Result<Self, Self::Error> {
        Ok(CephVmVolumeCommitMetadata {
            snap_name: record.snap_name.parse()?,
        })
    }
}

impl Into<RecordCephVolumeCommit> for CephVmVolumeCommitMetadata {
    fn into(self) -> RecordCephVolumeCommit {
        RecordCephVolumeCommit {
            snap_name: self.snap_name.to_string(),
        }
    }
}

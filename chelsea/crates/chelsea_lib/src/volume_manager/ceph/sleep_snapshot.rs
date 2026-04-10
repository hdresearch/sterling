use serde::{Deserialize, Serialize};
use vers_pg::schema::chelsea::tables::sleep_snapshot::RecordCephVolumeSleepSnapshot;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CephVmVolumeSleepSnapshotMetadata {
    pub image_name: String,
}

impl TryFrom<RecordCephVolumeSleepSnapshot> for CephVmVolumeSleepSnapshotMetadata {
    type Error = String;
    fn try_from(record: RecordCephVolumeSleepSnapshot) -> Result<Self, Self::Error> {
        Ok(CephVmVolumeSleepSnapshotMetadata {
            image_name: record.image_name,
        })
    }
}

impl Into<RecordCephVolumeSleepSnapshot> for CephVmVolumeSleepSnapshotMetadata {
    fn into(self) -> RecordCephVolumeSleepSnapshot {
        RecordCephVolumeSleepSnapshot {
            image_name: self.image_name,
        }
    }
}

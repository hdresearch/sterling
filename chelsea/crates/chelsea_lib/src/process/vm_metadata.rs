use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Contains information that users of the VM can use to introspect. It is the responsibility of individual VM process backends to ensure this is written to the expected location.
/// For example: /etc/vminfo
#[derive(Serialize, Deserialize)]
pub struct VmMetadata {
    pub vm_id: Uuid,
}

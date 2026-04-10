use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Events that may be sent by the VM to the host
pub enum VmEvent {
    Ready,
}

/// Minimized VM information relevant to the frontend
#[derive(ToSchema, Serialize, Deserialize, Debug)]
pub struct VmSummary {
    pub vm_id: String,
    pub state: VmState,
}

/// The state of a VM
#[derive(ToSchema, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VmState {
    Booting,
    Running,
    Paused,
    Sleeping,
    /// The VM process has crashed or is unreachable
    Dead,
}

pub struct VmReservation {
    pub vcpu_count: VmReservationField,
    pub memory_mib: VmReservationField,
    pub volume_mib: VmReservationField,
}

/// Represents a generic struct containing a max, total, and used reservation. Includes an accessor method for
/// available reservation.
pub struct VmReservationField {
    /// The per-VM hard maximum
    pub max: u32,
    /// The total amount reserved on the host machine
    pub total: u32,
    /// The amount currently reserved for VMs
    pub used: u32,
}

impl VmReservationField {
    pub fn available(&self) -> u32 {
        self.total.saturating_sub(self.used)
    }
}

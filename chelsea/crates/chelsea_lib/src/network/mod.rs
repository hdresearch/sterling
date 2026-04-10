/// A low-level networking module. Consuming code probably wishes to use network_manager instead
pub mod error;
pub mod linux;
pub mod utils;
mod vm_network;

pub use vm_network::{TAP_NAME, TAP_NET_V4, TAP_NET_V6, VmNetwork};

//! Cephalopod — native Rust bindings to librados and librbd.
//!
//! Replaces shelling out to the `rbd` CLI with direct FFI calls
//! for all operations. Device map/unmap use direct sysfs writes
//! to the kernel krbd module.

mod client;
pub mod compat;
mod default;
mod error;
mod ffi;
mod handle_cache;
mod rados;
pub mod rbd;
mod snap_name;
mod volume;

pub use client::Client;
pub use default::default_client;
pub use error::CephalopodError;
pub use rados::{RadosCluster, RadosIoCtx};
pub use rbd::{ChildInfo, ImageInfo, RbdImage, SnapInfo, WatcherInfo};
pub use snap_name::RbdSnapName;
pub use volume::ThinVolume;

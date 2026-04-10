// FFI bindings for librbd — only the subset we use.
#![allow(non_camel_case_types, dead_code)]

use std::os::raw::{c_char, c_int, c_void};

use super::rados::rados_ioctx_t;

pub type rbd_image_t = *mut c_void;
pub type rbd_image_options_t = *mut c_void;

// --- Structs ---

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct rbd_image_spec_t {
    pub id: *mut c_char,
    pub name: *mut c_char,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct rbd_linked_image_spec_t {
    pub pool_id: i64,
    pub pool_name: *mut c_char,
    pub pool_namespace: *mut c_char,
    pub image_id: *mut c_char,
    pub image_name: *mut c_char,
    pub trash: bool,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct rbd_snap_info_t {
    pub id: u64,
    pub size: u64,
    pub name: *const c_char,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct rbd_image_info_t {
    pub size: u64,
    pub obj_size: u64,
    pub num_objs: u64,
    pub order: c_int,
    pub block_name_prefix: [c_char; 24],
    pub parent_pool: i64,
    pub parent_name: [c_char; 96],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct rbd_image_watcher_t {
    pub addr: *mut c_char,
    pub id: i64,
    pub cookie: u64,
}

// --- Image option constants ---

pub const RBD_IMAGE_OPTION_FORMAT: c_int = 0;
pub const RBD_IMAGE_OPTION_FEATURES: c_int = 1;
pub const RBD_IMAGE_OPTION_ORDER: c_int = 2;
pub const RBD_IMAGE_OPTION_STRIPE_UNIT: c_int = 3;
pub const RBD_IMAGE_OPTION_STRIPE_COUNT: c_int = 4;
pub const RBD_IMAGE_OPTION_CLONE_FORMAT: c_int = 12;

// --- Image options ---

unsafe extern "C" {
    pub fn rbd_image_options_create(opts: *mut rbd_image_options_t);
    pub fn rbd_image_options_destroy(opts: rbd_image_options_t);
    pub fn rbd_image_options_set_uint64(
        opts: rbd_image_options_t,
        optname: c_int,
        optval: u64,
    ) -> c_int;
    pub fn rbd_image_options_set_string(
        opts: rbd_image_options_t,
        optname: c_int,
        optval: *const c_char,
    ) -> c_int;
}

// --- Image list ---

unsafe extern "C" {
    pub fn rbd_list2(
        io: rados_ioctx_t,
        images: *mut rbd_image_spec_t,
        max_images: *mut usize,
    ) -> c_int;
    pub fn rbd_image_spec_list_cleanup(images: *mut rbd_image_spec_t, num_images: usize);
}

// --- Image CRUD ---

unsafe extern "C" {
    pub fn rbd_create4(
        io: rados_ioctx_t,
        name: *const c_char,
        size: u64,
        opts: rbd_image_options_t,
    ) -> c_int;

    pub fn rbd_remove(io: rados_ioctx_t, name: *const c_char) -> c_int;

    pub fn rbd_open(
        io: rados_ioctx_t,
        name: *const c_char,
        image: *mut rbd_image_t,
        snap_name: *const c_char,
    ) -> c_int;

    pub fn rbd_close(image: rbd_image_t) -> c_int;

    pub fn rbd_stat(image: rbd_image_t, info: *mut rbd_image_info_t, infosize: usize) -> c_int;

    pub fn rbd_get_size(image: rbd_image_t, size: *mut u64) -> c_int;

    pub fn rbd_resize2(
        image: rbd_image_t,
        size: u64,
        allow_shrink: bool,
        cb: Option<unsafe extern "C" fn(u64, u64, *mut c_void) -> c_int>,
        cbdata: *mut c_void,
    ) -> c_int;
}

// --- Snapshots ---

unsafe extern "C" {
    pub fn rbd_snap_list(
        image: rbd_image_t,
        snaps: *mut rbd_snap_info_t,
        max_snaps: *mut c_int,
    ) -> c_int;

    pub fn rbd_snap_list_end(snaps: *mut rbd_snap_info_t);

    pub fn rbd_snap_create(image: rbd_image_t, snapname: *const c_char) -> c_int;

    pub fn rbd_snap_remove(image: rbd_image_t, snapname: *const c_char) -> c_int;

    pub fn rbd_snap_protect(image: rbd_image_t, snap_name: *const c_char) -> c_int;

    pub fn rbd_snap_unprotect(image: rbd_image_t, snap_name: *const c_char) -> c_int;

    pub fn rbd_snap_is_protected(
        image: rbd_image_t,
        snap_name: *const c_char,
        is_protected: *mut c_int,
    ) -> c_int;

    // NOTE: rbd_snap_purge is not available in all librbd builds.
    // We implement purge in safe code instead (unprotect + remove each snap).
}

// --- Clone ---

unsafe extern "C" {
    pub fn rbd_clone3(
        p_ioctx: rados_ioctx_t,
        p_name: *const c_char,
        p_snapname: *const c_char,
        c_ioctx: rados_ioctx_t,
        c_name: *const c_char,
        c_opts: rbd_image_options_t,
    ) -> c_int;
}

// --- Children ---

unsafe extern "C" {
    pub fn rbd_list_children3(
        image: rbd_image_t,
        images: *mut rbd_linked_image_spec_t,
        max_images: *mut usize,
    ) -> c_int;

    pub fn rbd_linked_image_spec_list_cleanup(
        images: *mut rbd_linked_image_spec_t,
        num_images: usize,
    );
}

// --- Watchers ---

unsafe extern "C" {
    pub fn rbd_watchers_list(
        image: rbd_image_t,
        watchers: *mut rbd_image_watcher_t,
        max_watchers: *mut usize,
    ) -> c_int;

    pub fn rbd_watchers_list_cleanup(watchers: *mut rbd_image_watcher_t, num_watchers: usize);
}

// --- Namespaces ---

unsafe extern "C" {
    pub fn rbd_namespace_create(io: rados_ioctx_t, namespace_name: *const c_char) -> c_int;

    pub fn rbd_namespace_remove(io: rados_ioctx_t, namespace_name: *const c_char) -> c_int;

    pub fn rbd_namespace_list(
        io: rados_ioctx_t,
        namespace_names: *mut c_char,
        size: *mut usize,
    ) -> c_int;

    pub fn rbd_namespace_exists(
        io: rados_ioctx_t,
        namespace_name: *const c_char,
        exists: *mut bool,
    ) -> c_int;
}

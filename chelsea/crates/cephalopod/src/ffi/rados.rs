// FFI bindings for librados — only the subset we use.
#![allow(non_camel_case_types, dead_code)]

use std::os::raw::{c_char, c_int, c_void};

pub type rados_t = *mut c_void;
pub type rados_ioctx_t = *mut c_void;

unsafe extern "C" {
    pub fn rados_create(cluster: *mut rados_t, id: *const c_char) -> c_int;
    pub fn rados_conf_read_file(cluster: rados_t, path: *const c_char) -> c_int;
    pub fn rados_connect(cluster: rados_t) -> c_int;
    pub fn rados_shutdown(cluster: rados_t);

    pub fn rados_ioctx_create(
        cluster: rados_t,
        pool_name: *const c_char,
        ioctx: *mut rados_ioctx_t,
    ) -> c_int;
    pub fn rados_ioctx_destroy(io: rados_ioctx_t);
    pub fn rados_ioctx_set_namespace(io: rados_ioctx_t, nspace: *const c_char);

    pub fn rados_conf_get(
        cluster: rados_t,
        option: *const c_char,
        buf: *mut c_char,
        len: usize,
    ) -> c_int;
}

//! Microbenchmarks to isolate individual cost components.

use criterion::{Criterion, criterion_group, criterion_main};
use std::ffi::CString;
use std::mem;
use std::ptr;
use std::sync::OnceLock;

// Direct FFI access for microbenchmarking
#[allow(non_camel_case_types, dead_code)]
mod ffi {
    use std::os::raw::{c_char, c_int, c_void};
    pub type rados_t = *mut c_void;
    pub type rados_ioctx_t = *mut c_void;
    pub type rbd_image_t = *mut c_void;

    #[repr(C)]
    pub struct rbd_image_info_t {
        pub size: u64,
        pub obj_size: u64,
        pub num_objs: u64,
        pub order: c_int,
        pub block_name_prefix: [c_char; 24],
        pub parent_pool: i64,
        pub parent_name: [c_char; 96],
    }

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
        pub fn rbd_open(
            io: rados_ioctx_t,
            name: *const c_char,
            image: *mut rbd_image_t,
            snap_name: *const c_char,
        ) -> c_int;
        pub fn rbd_close(image: rbd_image_t) -> c_int;
        pub fn rbd_stat(image: rbd_image_t, info: *mut rbd_image_info_t, infosize: usize) -> c_int;
        pub fn rbd_get_size(image: rbd_image_t, size: *mut u64) -> c_int;
        pub fn rbd_namespace_exists(
            io: rados_ioctx_t,
            namespace_name: *const c_char,
            exists: *mut bool,
        ) -> c_int;
    }
}

struct RawCluster(ffi::rados_t);
unsafe impl Send for RawCluster {}
unsafe impl Sync for RawCluster {}

fn raw_cluster() -> &'static RawCluster {
    static CLUSTER: OnceLock<RawCluster> = OnceLock::new();
    CLUSTER.get_or_init(|| {
        let c_id = CString::new("chelsea").unwrap();
        let mut handle: ffi::rados_t = ptr::null_mut();
        unsafe {
            ffi::rados_create(&mut handle, c_id.as_ptr());
            ffi::rados_conf_read_file(handle, ptr::null());
            ffi::rados_connect(handle);
        }
        RawCluster(handle)
    })
}

fn make_ioctx() -> ffi::rados_ioctx_t {
    let c_pool = CString::new("rbd").unwrap();
    let mut ioctx: ffi::rados_ioctx_t = ptr::null_mut();
    unsafe {
        ffi::rados_ioctx_create(raw_cluster().0, c_pool.as_ptr(), &mut ioctx);
    }
    ioctx
}

fn ensure_bench_image() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let c = cephalopod::Client::connect("chelsea", "rbd").unwrap();
        if !c.image_exists("cephalopod-micro-bench").await.unwrap() {
            c.image_create("cephalopod-micro-bench", 32).await.unwrap();
        }
    });
}

// ---------------------------------------------------------------------------
// Microbenchmarks
// ---------------------------------------------------------------------------

fn bench_ioctx_create_destroy(c: &mut Criterion) {
    let _ = raw_cluster(); // ensure connected

    c.bench_function("µ: ioctx_create + ioctx_destroy", |b| {
        let c_pool = CString::new("rbd").unwrap();
        b.iter(|| {
            let mut ioctx: ffi::rados_ioctx_t = ptr::null_mut();
            unsafe {
                ffi::rados_ioctx_create(raw_cluster().0, c_pool.as_ptr(), &mut ioctx);
                ffi::rados_ioctx_destroy(ioctx);
            }
        });
    });
}

fn bench_ioctx_set_namespace(c: &mut Criterion) {
    let ioctx = make_ioctx();
    let c_ns = CString::new("some-namespace").unwrap();

    c.bench_function("µ: ioctx_set_namespace", |b| {
        b.iter(|| unsafe {
            ffi::rados_ioctx_set_namespace(ioctx, c_ns.as_ptr());
        });
    });

    unsafe { ffi::rados_ioctx_destroy(ioctx) };
}

fn bench_rbd_open_close(c: &mut Criterion) {
    ensure_bench_image();
    let ioctx = make_ioctx();
    let c_name = CString::new("cephalopod-micro-bench").unwrap();

    c.bench_function("µ: rbd_open + rbd_close", |b| {
        b.iter(|| {
            let mut handle: ffi::rbd_image_t = ptr::null_mut();
            unsafe {
                ffi::rbd_open(ioctx, c_name.as_ptr(), &mut handle, ptr::null());
                ffi::rbd_close(handle);
            }
        });
    });

    unsafe { ffi::rados_ioctx_destroy(ioctx) };
}

fn bench_rbd_stat_with_open(c: &mut Criterion) {
    ensure_bench_image();
    let ioctx = make_ioctx();
    let c_name = CString::new("cephalopod-micro-bench").unwrap();

    c.bench_function("µ: rbd_open + rbd_stat + rbd_close", |b| {
        b.iter(|| {
            let mut handle: ffi::rbd_image_t = ptr::null_mut();
            let mut info: ffi::rbd_image_info_t = unsafe { mem::zeroed() };
            unsafe {
                ffi::rbd_open(ioctx, c_name.as_ptr(), &mut handle, ptr::null());
                ffi::rbd_stat(handle, &mut info, mem::size_of::<ffi::rbd_image_info_t>());
                ffi::rbd_close(handle);
            }
        });
    });

    unsafe { ffi::rados_ioctx_destroy(ioctx) };
}

fn bench_rbd_stat_cached_handle(c: &mut Criterion) {
    ensure_bench_image();
    let ioctx = make_ioctx();
    let c_name = CString::new("cephalopod-micro-bench").unwrap();

    // Open once, reuse handle
    let mut handle: ffi::rbd_image_t = ptr::null_mut();
    unsafe {
        ffi::rbd_open(ioctx, c_name.as_ptr(), &mut handle, ptr::null());
    }

    c.bench_function("µ: rbd_stat (cached handle)", |b| {
        b.iter(|| {
            let mut info: ffi::rbd_image_info_t = unsafe { mem::zeroed() };
            unsafe {
                ffi::rbd_stat(handle, &mut info, mem::size_of::<ffi::rbd_image_info_t>());
            }
        });
    });

    unsafe {
        ffi::rbd_close(handle);
        ffi::rados_ioctx_destroy(ioctx);
    };
}

fn bench_rbd_get_size_cached_handle(c: &mut Criterion) {
    ensure_bench_image();
    let ioctx = make_ioctx();
    let c_name = CString::new("cephalopod-micro-bench").unwrap();

    let mut handle: ffi::rbd_image_t = ptr::null_mut();
    unsafe {
        ffi::rbd_open(ioctx, c_name.as_ptr(), &mut handle, ptr::null());
    }

    c.bench_function("µ: rbd_get_size (cached handle)", |b| {
        b.iter(|| {
            let mut size: u64 = 0;
            unsafe {
                ffi::rbd_get_size(handle, &mut size);
            }
        });
    });

    unsafe {
        ffi::rbd_close(handle);
        ffi::rados_ioctx_destroy(ioctx);
    };
}

fn bench_namespace_exists_cached_ioctx(c: &mut Criterion) {
    let ioctx = make_ioctx();
    // Ensure namespace exists
    let _ = tokio::runtime::Runtime::new().unwrap().block_on(async {
        let c = cephalopod::Client::connect("chelsea", "rbd").unwrap();
        let _ = c.namespace_create("cephalopod-micro-ns").await;
    });
    let c_ns = CString::new("cephalopod-micro-ns").unwrap();

    c.bench_function("µ: namespace_exists (cached ioctx)", |b| {
        b.iter(|| {
            let mut exists = false;
            unsafe {
                ffi::rbd_namespace_exists(ioctx, c_ns.as_ptr(), &mut exists);
            }
        });
    });

    unsafe { ffi::rados_ioctx_destroy(ioctx) };
}

fn bench_spawn_blocking_overhead(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("µ: spawn_blocking (noop)", |b| {
        b.to_async(&rt).iter(|| async {
            tokio::task::spawn_blocking(|| {}).await.unwrap();
        });
    });
}

fn bench_cstring_alloc(c: &mut Criterion) {
    c.bench_function("µ: CString::new (short)", |b| {
        b.iter(|| {
            let _ = CString::new("cephalopod-micro-bench").unwrap();
        });
    });

    c.bench_function("µ: CString::new (uuid-like)", |b| {
        b.iter(|| {
            let _ = CString::new("550e8400-e29b-41d4-a716-446655440000").unwrap();
        });
    });
}

criterion_group!(
    name = micro;
    config = Criterion::default().sample_size(100);
    targets =
        bench_ioctx_create_destroy,
        bench_ioctx_set_namespace,
        bench_rbd_open_close,
        bench_rbd_stat_with_open,
        bench_rbd_stat_cached_handle,
        bench_rbd_get_size_cached_handle,
        bench_namespace_exists_cached_ioctx,
        bench_spawn_blocking_overhead,
        bench_cstring_alloc
);

criterion_main!(micro);

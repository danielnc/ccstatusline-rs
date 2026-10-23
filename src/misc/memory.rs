//! System memory for the free-memory widget, computed natively.
//!
//! Upstream (`src/widgets/FreeMemory.ts`) uses `os.totalmem()`, and on macOS
//! runs `vm_stat` for used = (active + wired pages) * page size, falling back
//! to `total - os.freemem()`. `vm_stat` reads the same `host_statistics64`
//! counters and `host_page_size` we call here, so the numbers match without
//! the subprocess.
//!
//! The parity harness pins the values: `CCSL_TEST_MEM_TOTAL_BYTES` replaces
//! `os.totalmem()` and makes `vm_stat` fail, `CCSL_TEST_MEM_FREE_BYTES`
//! replaces `os.freemem()` (see parity/preload.mjs).

pub struct Memory {
    pub used: f64,
    pub total: f64,
}

fn env_number(name: &str) -> Option<f64> {
    // `if (env.X) Number(env.X)`: a non-empty value, NaN allowed
    let v = std::env::var(name).ok().filter(|v| !v.is_empty())?;
    Some(crate::jsnum::number_from_str(&v).unwrap_or(f64::NAN))
}

pub fn read() -> Memory {
    let total_override = env_number("CCSL_TEST_MEM_TOTAL_BYTES");
    let total = total_override.unwrap_or_else(total_mem);
    let free = || env_number("CCSL_TEST_MEM_FREE_BYTES").unwrap_or_else(free_mem);
    let native_used = if total_override.is_none() { used_mem_vm_stat() } else { None };
    let used = native_used.unwrap_or_else(|| total - free());
    Memory { used, total }
}

#[cfg(target_os = "macos")]
fn total_mem() -> f64 {
    let mut v: u64 = 0;
    let mut len = std::mem::size_of::<u64>();
    let mut mib = [libc::CTL_HW, libc::HW_MEMSIZE];
    let rc = unsafe {
        libc::sysctl(mib.as_mut_ptr(), 2, &mut v as *mut u64 as *mut libc::c_void, &mut len, std::ptr::null_mut(), 0)
    };
    if rc == 0 {
        v as f64
    } else {
        0.0
    }
}

#[cfg(target_os = "macos")]
extern "C" {
    fn host_page_size(host: libc::mach_port_t, size: *mut libc::vm_size_t) -> libc::kern_return_t;
}

#[cfg(target_os = "macos")]
fn vm_stats() -> Option<libc::vm_statistics64> {
    unsafe {
        let mut info: libc::vm_statistics64 = std::mem::zeroed();
        let mut count = libc::HOST_VM_INFO64_COUNT;
        #[allow(deprecated)]
        let host = libc::mach_host_self();
        let rc = libc::host_statistics64(host, libc::HOST_VM_INFO64, &mut info as *mut _ as *mut i32, &mut count);
        (rc == libc::KERN_SUCCESS).then_some(info)
    }
}

/// libuv `uv_get_free_memory`: free pages * sysconf(_SC_PAGESIZE).
#[cfg(target_os = "macos")]
fn free_mem() -> f64 {
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as f64;
    vm_stats().map(|i| i.free_count as f64 * page).unwrap_or(0.0)
}

/// What `vm_stat` prints: (Pages active + Pages wired down) * page size.
#[cfg(target_os = "macos")]
fn used_mem_vm_stat() -> Option<f64> {
    let info = vm_stats()?;
    let mut page: libc::vm_size_t = 0;
    #[allow(deprecated)]
    let rc = unsafe { host_page_size(libc::mach_host_self(), &mut page) };
    let page = if rc == libc::KERN_SUCCESS { page as f64 } else { 4096.0 };
    Some((info.active_count as f64 + info.wire_count as f64) * page)
}

#[cfg(target_os = "linux")]
fn meminfo_kb(key: &str) -> Option<f64> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = text.lines().find(|l| l.starts_with(key))?;
    let n: f64 = line[key.len()..].trim().trim_end_matches("kB").trim().parse().ok()?;
    Some(n * 1024.0)
}

#[cfg(target_os = "linux")]
fn total_mem() -> f64 {
    meminfo_kb("MemTotal:").unwrap_or(0.0)
}

#[cfg(target_os = "linux")]
fn free_mem() -> f64 {
    meminfo_kb("MemAvailable:").unwrap_or(0.0)
}

#[cfg(not(target_os = "macos"))]
fn used_mem_vm_stat() -> Option<f64> {
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn total_mem() -> f64 {
    0.0
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn free_mem() -> f64 {
    0.0
}

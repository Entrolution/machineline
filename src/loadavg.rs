//! 1-minute load average (libc `getloadavg`) and the logical CPU count, so the line can show load
//! relative to cores — the oversubscription gauge. `getloadavg` lives in libSystem on macOS and
//! libc on Linux/BSD; no spawn, no extra crate.

/// Logical CPU count (cores × SMT threads), or 1 if it can't be determined.
pub fn ncpu() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

/// The 1-minute load average, or `None` if the platform can't supply it.
#[cfg(unix)]
pub fn load1() -> Option<f64> {
    extern "C" {
        fn getloadavg(loadavg: *mut f64, nelem: std::os::raw::c_int) -> std::os::raw::c_int;
    }
    let mut avg = [0.0f64; 3];
    // SAFETY: `getloadavg` writes at most `nelem` doubles into the buffer; we pass a 3-element
    // array and request 3. It returns the count written (≥1) or -1 on failure.
    let n = unsafe { getloadavg(avg.as_mut_ptr(), 3) };
    if n >= 1 {
        Some(avg[0])
    } else {
        None
    }
}

#[cfg(not(unix))]
pub fn load1() -> Option<f64> {
    None
}

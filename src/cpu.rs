//! Instantaneous CPU utilisation from the kernel's cumulative tick counters (mach
//! `host_statistics` / `HOST_CPU_LOAD_INFO`). The counters are monotonic since boot, so a single
//! read is meaningless — utilisation is the *delta* between two reads. We stash the previous
//! counters in the cache and compute the busy fraction across successive `refresh` runs, which
//! avoids ever sleeping on the render path to take a sample.

/// Cumulative CPU ticks since boot, summed across all logical CPUs.
#[derive(Clone, Copy)]
pub struct Ticks {
    pub busy: u64, // user + system + nice
    pub idle: u64,
}

/// Read the current cumulative tick counters, or `None` if the mach call fails / is unsupported.
#[cfg(target_os = "macos")]
pub fn ticks() -> Option<Ticks> {
    const HOST_CPU_LOAD_INFO: i32 = 3;
    const COUNT: u32 = 4; // HOST_CPU_LOAD_INFO_COUNT, in natural_t units
    const USER: usize = 0;
    const SYSTEM: usize = 1;
    const IDLE: usize = 2;
    const NICE: usize = 3;

    type HostT = u32; // mach_port_t
    extern "C" {
        fn mach_host_self() -> HostT;
        fn host_statistics(host: HostT, flavor: i32, info: *mut u32, count: *mut u32) -> i32;
    }

    let mut data = [0u32; 4];
    let mut count = COUNT;
    // SAFETY: `host_statistics` writes `count` natural_t (u32) values into `info`; we pass a
    // 4-element buffer and COUNT=4, the documented size for HOST_CPU_LOAD_INFO. The host port is a
    // cheap send right; this process is short-lived so we don't bother deallocating it.
    let kr = unsafe {
        let host = mach_host_self();
        host_statistics(host, HOST_CPU_LOAD_INFO, data.as_mut_ptr(), &mut count)
    };
    if kr != 0 {
        return None;
    }
    Some(Ticks {
        busy: data[USER] as u64 + data[SYSTEM] as u64 + data[NICE] as u64,
        idle: data[IDLE] as u64,
    })
}

#[cfg(not(target_os = "macos"))]
pub fn ticks() -> Option<Ticks> {
    None
}

/// Utilisation percent between two cumulative readings (`prev` older). `None` if the counters
/// didn't advance (no elapsed time) or went backwards (counter wrap / reset).
pub fn utilization(prev: Ticks, cur: Ticks) -> Option<f64> {
    let busy = cur.busy.checked_sub(prev.busy)?;
    let idle = cur.idle.checked_sub(prev.idle)?;
    let total = busy + idle;
    if total == 0 {
        return None;
    }
    Some(busy as f64 / total as f64 * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn util_delta() {
        let a = Ticks {
            busy: 100,
            idle: 900,
        };
        let b = Ticks {
            busy: 300,
            idle: 1700,
        }; // +200 busy, +800 idle → 20%
        assert_eq!(utilization(a, b), Some(20.0));
    }

    #[test]
    fn util_guards() {
        let a = Ticks {
            busy: 100,
            idle: 900,
        };
        assert_eq!(utilization(a, a), None); // no elapsed ticks
        let backwards = Ticks {
            busy: 50,
            idle: 800,
        };
        assert_eq!(utilization(a, backwards), None); // counter went backwards
    }
}

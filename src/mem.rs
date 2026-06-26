//! Memory: how full RAM is, the kernel's memory-pressure level (the honest signal on macOS, where
//! lots of RAM is held as reclaimable cache so raw "used" overstates things), and swap in use —
//! the thrashing tell. Read from `vm_stat` (page counts + page size) and `sysctl` (swap, pressure).

use crate::sys::output;

#[derive(Default)]
pub struct Mem {
    /// RAM in active use as a percent of physical (wired + active + compressed).
    pub used_pct: Option<f64>,
    /// Kernel memory-pressure level: 1 = normal, 2 = warn, 4 = critical (macOS).
    pub pressure: Option<u8>,
    /// Bytes of swap currently in use.
    pub swap_used: Option<f64>,
}

pub fn read() -> Mem {
    Mem {
        used_pct: output("vm_stat", &[]).as_deref().and_then(parse_vm_stat),
        pressure: output("sysctl", &["-n", "kern.memorystatus_vm_pressure_level"])
            .as_deref()
            .and_then(parse_pressure),
        swap_used: output("sysctl", &["-n", "vm.swapusage"])
            .as_deref()
            .and_then(parse_swap_used),
    }
}

/// `vm_stat` → "Memory Used"-style percent: (wired + active + compressed) / total pages, where
/// total = free + active + inactive + speculative + wired + compressed. Page counts only; the page
/// size cancels out of the ratio, so we never need to read it.
pub fn parse_vm_stat(text: &str) -> Option<f64> {
    let pages = |label: &str| -> Option<f64> {
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix(label) {
                let digits: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
                return digits.parse::<f64>().ok();
            }
        }
        None
    };
    let free = pages("Pages free:")?;
    let active = pages("Pages active:")?;
    let inactive = pages("Pages inactive:")?;
    let spec = pages("Pages speculative:").unwrap_or(0.0);
    let wired = pages("Pages wired down:")?;
    let compressed = pages("Pages occupied by compressor:").unwrap_or(0.0);
    let total = free + active + inactive + spec + wired + compressed;
    if total <= 0.0 {
        return None;
    }
    Some((wired + active + compressed) / total * 100.0)
}

pub fn parse_pressure(text: &str) -> Option<u8> {
    text.trim().parse().ok()
}

/// `vm.swapusage` → "total = 3072.00M  used = 2457.19M  free = 614.81M ..." → used bytes.
pub fn parse_swap_used(text: &str) -> Option<f64> {
    let after = text.split("used =").nth(1)?.trim();
    parse_size_token(after.split_whitespace().next()?)
}

/// "2457.19M" / "3.00G" / "512.00K" / "0.00M" → bytes.
fn parse_size_token(tok: &str) -> Option<f64> {
    let (num, mult) = if let Some(n) = tok.strip_suffix('G') {
        (n, 1024.0 * 1024.0 * 1024.0)
    } else if let Some(n) = tok.strip_suffix('M') {
        (n, 1024.0 * 1024.0)
    } else if let Some(n) = tok.strip_suffix('K') {
        (n, 1024.0)
    } else {
        (tok, 1.0)
    };
    num.parse::<f64>().ok().map(|v| v * mult)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VM_STAT: &str = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\n\
        Pages free:                  100000.\n\
        Pages active:                300000.\n\
        Pages inactive:              200000.\n\
        Pages speculative:            50000.\n\
        Pages wired down:            150000.\n\
        Pages occupied by compressor: 50000.\n";

    #[test]
    fn vm_stat_used_pct() {
        // used = wired+active+compressed = 150000+300000+50000 = 500000
        // total = 100000+300000+200000+50000+150000+50000 = 850000 → ~58.8%
        let p = parse_vm_stat(VM_STAT).unwrap();
        assert!((p - 58.82).abs() < 0.1, "{p}");
    }

    #[test]
    fn swap_used_bytes() {
        let t = "total = 3072.00M  used = 2457.19M  free = 614.81M  (encrypted)";
        let b = parse_swap_used(t).unwrap();
        assert!((b - 2457.19 * 1024.0 * 1024.0).abs() < 1.0, "{b}");
        assert_eq!(
            parse_swap_used("total = 0.00M  used = 0.00M  free = 0.00M"),
            Some(0.0)
        );
    }

    #[test]
    fn pressure_level() {
        assert_eq!(parse_pressure(" 1\n"), Some(1));
        assert_eq!(parse_pressure("4"), Some(4));
    }
}

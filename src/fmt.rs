//! ANSI colours and the value formatters shared by the status line and the `check` command.
//! Colour constants are lifted from quotaline/vastline so the three status lines match visually.

pub const RESET: &str = "\x1b[0m";
pub const DIM: &str = "\x1b[2m";
pub const GRAY: &str = "\x1b[90m";
pub const GREEN: &str = "\x1b[32m";
pub const AMBER: &str = "\x1b[38;5;214m"; // 256-colour amber (warning band)
pub const RED: &str = "\x1b[31m";

/// Colour for an *ascending-bad* metric (bigger = worse): green below `amber`, amber up to
/// `red`, red at/above `red`. The everyday shape for load, utilisation, swap and temperature.
pub fn band_up(v: f64, amber: f64, red: f64) -> &'static str {
    if v >= red {
        RED
    } else if v >= amber {
        AMBER
    } else {
        GREEN
    }
}

/// Throttle / CPU speed-limit colour: 100% is healthy (green), anything less is the machine
/// clamping itself (amber), a deep clamp is red. Inverse of `band_up` — smaller is worse.
pub fn band_throttle(limit: u32) -> &'static str {
    if limit >= 100 {
        GREEN
    } else if limit > RED_THROTTLE_AT {
        AMBER
    } else {
        RED
    }
}

/// At or below this speed-limit %, the throttle reads red rather than amber.
pub const RED_THROTTLE_AT: u32 = 50;

/// Bytes → compact `2.4G`, `512M`, `64K`, `0`. Binary units; one decimal in the G/M band.
pub fn fmt_bytes(n: f64) -> String {
    const K: f64 = 1024.0;
    const M: f64 = K * 1024.0;
    const G: f64 = M * 1024.0;
    if n <= 0.0 {
        "0".to_string()
    } else if n >= G {
        format!("{:.1}G", n / G)
    } else if n >= M {
        format!("{:.0}M", n / M)
    } else if n >= K {
        format!("{:.0}K", n / K)
    } else {
        format!("{}B", n as i64)
    }
}

/// Load average → `0.5`, `6.5`, `12` (one decimal, trailing `.0` trimmed).
pub fn fmt_load(v: f64) -> String {
    let s = format!("{v:.1}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands() {
        assert_eq!(band_up(0.3, 0.7, 1.0), GREEN);
        assert_eq!(band_up(0.8, 0.7, 1.0), AMBER);
        assert_eq!(band_up(1.5, 0.7, 1.0), RED);
        assert_eq!(band_throttle(100), GREEN);
        assert_eq!(band_throttle(80), AMBER);
        assert_eq!(band_throttle(20), RED);
        assert_eq!(band_throttle(50), RED); // boundary: ≤50 is red
    }

    #[test]
    fn bytes() {
        assert_eq!(fmt_bytes(0.0), "0");
        assert_eq!(fmt_bytes(2.4 * 1024.0 * 1024.0 * 1024.0), "2.4G");
        assert_eq!(fmt_bytes(512.0 * 1024.0 * 1024.0), "512M");
        assert_eq!(fmt_bytes(64.0 * 1024.0), "64K");
    }

    #[test]
    fn load() {
        assert_eq!(fmt_load(0.5), "0.5");
        assert_eq!(fmt_load(6.53), "6.5");
        assert_eq!(fmt_load(12.0), "12");
    }
}

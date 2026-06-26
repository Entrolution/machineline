//! Power / battery, via `pmset -g batt` (state of charge, AC vs battery, charging) and the adapter
//! wattage from `pmset -g ac`. The genuinely useful warning is **"on AC but discharging"** — the
//! under-powered-adapter trap — which `pmset` reports as "AC attached; discharging".

use crate::sys::output;

#[derive(Default)]
pub struct Power {
    pub charge_pct: Option<f64>,
    pub on_ac: Option<bool>,
    pub charging: Option<bool>,
    /// On wall power yet still draining the battery — the adapter can't keep up under load.
    pub draining_on_ac: bool,
    pub adapter_w: Option<u32>,
}

pub fn read() -> Power {
    let mut p = parse_batt(&output("pmset", &["-g", "batt"]).unwrap_or_default());
    p.adapter_w = output("pmset", &["-g", "ac"])
        .as_deref()
        .and_then(parse_adapter_watts);
    p
}

/// Parse `pmset -g batt`. Header is "Now drawing from 'AC Power'" / "'Battery Power'"; the body
/// line carries the percentage and a state phrase ("charging" / "not charging" / "discharging" /
/// "charged"). Pure, for testing.
pub fn parse_batt(text: &str) -> Power {
    let mut p = Power::default();
    if text.contains("'AC Power'") {
        p.on_ac = Some(true);
    } else if text.contains("'Battery Power'") {
        p.on_ac = Some(false);
    }
    p.charge_pct = find_percent(text);

    let lower = text.to_ascii_lowercase();
    // Order matters: "not charging" contains "charging", so test the negatives first.
    p.charging = if lower.contains("discharging") || lower.contains("not charging") {
        Some(false)
    } else if lower.contains("charging") || lower.contains("charged") {
        Some(true)
    } else {
        None
    };
    p.draining_on_ac = p.on_ac == Some(true) && lower.contains("discharging");
    p
}

/// First `NN%` token in the text → charge percentage.
fn find_percent(text: &str) -> Option<f64> {
    let bytes = text.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'%' {
            let mut j = i;
            while j > 0 && bytes[j - 1].is_ascii_digit() {
                j -= 1;
            }
            if j < i {
                return text[j..i].parse::<f64>().ok();
            }
        }
    }
    None
}

/// `pmset -g ac` → "Wattage = 96W" → 96.
pub fn parse_adapter_watts(text: &str) -> Option<u32> {
    for line in text.lines() {
        if let Some((_, rest)) = line.split_once("Wattage") {
            let digits: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                return digits.parse().ok();
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ac_not_charging_is_not_draining() {
        // Optimised-charging hold at 80% — on AC, "not charging", but NOT draining.
        let t = "Now drawing from 'AC Power'\n \
            -InternalBattery-0 (id=123)\t80%; AC attached; not charging present: true";
        let p = parse_batt(t);
        assert_eq!(p.on_ac, Some(true));
        assert_eq!(p.charge_pct, Some(80.0));
        assert_eq!(p.charging, Some(false));
        assert!(!p.draining_on_ac);
    }

    #[test]
    fn ac_discharging_flags_underpowered() {
        let t = "Now drawing from 'AC Power'\n \
            -InternalBattery-0 (id=123)\t77%; AC attached; discharging present: true";
        let p = parse_batt(t);
        assert!(
            p.draining_on_ac,
            "AC + discharging must flag the under-powered adapter"
        );
    }

    #[test]
    fn on_battery_charging_state() {
        let t = "Now drawing from 'Battery Power'\n \
            -InternalBattery-0 (id=123)\t64%; discharging; 3:12 remaining present: true";
        let p = parse_batt(t);
        assert_eq!(p.on_ac, Some(false));
        assert_eq!(p.charging, Some(false));
        assert!(!p.draining_on_ac); // not on AC, so not the warning case
    }

    #[test]
    fn adapter_watts() {
        assert_eq!(
            parse_adapter_watts(" Wattage = 96W\n Current = 4800mA"),
            Some(96)
        );
        assert_eq!(parse_adapter_watts("no adapter"), None);
    }
}

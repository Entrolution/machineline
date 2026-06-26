//! The single hottest process right now, via `ps -Ao %cpu=,comm= -r` (sorted by CPU, descending,
//! headers suppressed by the trailing `=`). `%cpu` is normalised to one core, so a multi-threaded
//! hog can read above 100%.

use crate::sys::output;

#[derive(Clone)]
pub struct TopProc {
    pub name: String,
    pub cpu: f64,
}

pub fn top() -> Option<TopProc> {
    parse(&output("ps", &["-Ao", "%cpu=,comm=", "-r"])?)
}

/// First non-empty line of the sorted `ps` output → the hottest process.
pub fn parse(text: &str) -> Option<TopProc> {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    let (cpu_str, rest) = line.split_once(char::is_whitespace)?;
    let cpu: f64 = cpu_str.parse().ok()?;
    let name = basename(rest.trim());
    if name.is_empty() {
        None
    } else {
        Some(TopProc { name, cpu })
    }
}

/// Last path component of a `comm` (macOS reports it as a full executable path).
fn basename(comm: &str) -> String {
    comm.rsplit('/').next().unwrap_or(comm).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_first_and_basenames() {
        let out = " 61.3 /Applications/iTerm.app/Contents/MacOS/iTerm2\n 47.0 /System/Library/.../WindowServer\n";
        let p = parse(out).unwrap();
        assert_eq!(p.name, "iTerm2");
        assert!((p.cpu - 61.3).abs() < 1e-9);
    }

    #[test]
    fn handles_bare_comm() {
        let p = parse("12.5 kernel_task").unwrap();
        assert_eq!(p.name, "kernel_task");
        assert!((p.cpu - 12.5).abs() < 1e-9);
    }
}

//! machineline — a Claude Code status line for local-machine health: CPU speed-limit/throttle,
//! load, utilisation, memory pressure, swap, temperature, the hottest process, and power.
//!
//!   machineline                       render the status line (reads/forwards Claude's JSON stdin)
//!   machineline refresh               gather metrics and rewrite the cache (run by the bg refresh)
//!   machineline check                 gather once and print the line + raw values (debug)
//!   machineline install [--refresh N] wire into ~/.claude/settings.json (delegates to any existing
//!                                     status line, e.g. vastline/quotaline)
//!   machineline uninstall [--purge]   restore the previous status line; --purge also drops the cache
//!
//! Everything is read from the OS (mach/libc, the IOKit SMC, and stock CLIs). Process spawns happen
//! only in `refresh`; the render path reads a cache, so the prompt never blocks.

mod cache;
mod config;
mod cpu;
mod fmt;
mod install;
mod loadavg;
mod mem;
mod power;
mod proc;
mod render;
mod sys;
mod temp;
mod throttle;

const USAGE: &str = "\
machineline — Claude Code status line for local-machine health

USAGE:
  machineline                        render the status line (reads Claude Code's JSON on stdin)
  machineline refresh                gather metrics and rewrite the cache
  machineline check                  gather once and print the line + raw values (debug)
  machineline install [--refresh N]  wire into ~/.claude/settings.json (refresh seconds; default 10)
  machineline uninstall [--purge]    restore the previous status line; --purge drops the cache
";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let code = match args.get(1).map(String::as_str) {
        None => render::run_statusline(),
        Some("refresh") => cache::run_refresh(),
        Some("check") => check(),
        Some("install") => {
            let refresh = flag_u64(&args, "--refresh").unwrap_or(10);
            install::install(refresh)
        }
        Some("uninstall") => install::uninstall(has_flag(&args, "--purge")),
        Some("-h") | Some("--help") | Some("help") => {
            print!("{USAGE}");
            0
        }
        Some(other) => {
            eprintln!("machineline: unknown command '{other}'\n");
            eprint!("{USAGE}");
            2
        }
    };
    std::process::exit(code);
}

fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn flag_u64(args: &[String], name: &str) -> Option<u64> {
    let i = args.iter().position(|a| a == name)?;
    args.get(i + 1).and_then(|s| s.parse::<u64>().ok())
}

/// `machineline check` — gather two samples ~300ms apart (so CPU utilisation has a delta to report)
/// and print the rendered line plus the raw values, for debugging the readers.
fn check() -> i32 {
    let first = cache::gather(None, config::now_secs());
    std::thread::sleep(std::time::Duration::from_millis(300));
    let s = cache::gather(Some(&first), config::now_secs());

    println!("{}", render::line(Some(&s), config::now_secs()));
    println!("\nraw:");
    println!("  speed_limit : {:?}", s.speed_limit);
    println!("  load1 / ncpu: {:?} / {}", s.load1, s.ncpu);
    println!("  cpu_util    : {:?}", s.cpu_util);
    println!("  mem_used_%  : {:?}", s.mem_used_pct);
    println!("  mem_pressure: {:?}", s.mem_pressure);
    println!("  swap_used   : {:?}", s.swap_used);
    println!("  temp_c      : {:?}", s.temp_c);
    println!("  top         : {:?} {:?}", s.top_name, s.top_cpu);
    println!(
        "  power       : on_ac={:?} charging={:?} draining_on_ac={} adapter_w={:?} charge%={:?}",
        s.on_ac, s.charging, s.draining_on_ac, s.adapter_w, s.charge_pct
    );
    0
}

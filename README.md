# machineline

[![CI](https://github.com/Entrolution/machineline/actions/workflows/ci.yml/badge.svg)](https://github.com/Entrolution/machineline/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Entrolution/machineline?logo=github&color=2ea44f)](https://github.com/Entrolution/machineline/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![Platforms](https://img.shields.io/badge/platforms-macOS-555)
![Rust](https://img.shields.io/badge/rust-stdlib%20%2B%20serde-dea584?logo=rust)

A Claude Code **status line** for the health of the machine you're sitting at — the **CPU
speed-limit / thermal throttle**, load, utilisation, memory pressure, swap, temperature, the
hottest process, and power. It reads the OS directly and caches off the render path, so the
prompt never blocks.

![machineline status line — a healthy machine (clk 100%, load, cpu, mem, temperature, top process, AC adapter) and the same line on a hot day with the CPU throttled to clk 31%, swap in use, 96°C, and an under-powered-adapter warning — colour-banded green/amber/red](assets/demo.svg)

The top line is a healthy machine; the bottom is the same machine struggling on a hot day — the
throttle (`clk`) drops, swap appears, the temperature climbs, and the adapter warning lights up.

## Why machineline

- **The throttle is the headline.** Intel MacBooks clamp `CPU_Speed_Limit` to a fraction of rated
  speed under thermal/power stress — and when they do, *everything* feels floored while the CPU
  utilisation graph still reads 100% (utilisation is "how busy", not "how fast"). `clk` shows the
  real speed limit, so you see the clamp the moment it bites instead of guessing.
- **One line, the whole box.** Throttle, load-vs-cores, live utilisation, memory pressure, swap,
  CPU temperature, the current hog, and the power/adapter state — together, colour-banded.
- **It stacks.** machineline installs *on top of* whatever status line you already run, printing
  its line above [vastline](https://github.com/Entrolution/vastline) and
  [quotaline](https://github.com/Entrolution/quotaline) rather than replacing them.
- **One small binary.** A single ~480 KB native executable — no runtime, no dependencies beyond
  serde, and it installs itself.

## What it shows

Each segment appears only when the OS can supply it (so it degrades cleanly across machines):

- **`clk`** — CPU speed limit from `pmset -g therm` (`CPU_Speed_Limit`). 100% is unclamped (green);
  lower is the machine throttling itself (amber, then red at/below 50%). **macOS, Intel** — Apple
  Silicon doesn't report it, so the segment disappears.
- **`load`** — 1-minute load average, coloured by load ÷ logical-core count: green below 0.7×,
  amber to 1.0×, red when the run queue is oversubscribed.
- **`cpu`** — instantaneous utilisation (busy %), computed from the kernel's tick counters as a
  delta between samples — green/amber/red at 80/95%.
- **`mem`** — RAM in use as a percent, coloured by the kernel's **memory-pressure** level
  (normal/warn/critical) rather than the raw number, since macOS holds a lot of RAM as reclaimable
  cache.
- **`swap`** — swap in use, shown only when something is actually swapped out (the thrashing tell).
- **`NN°C`** — CPU temperature, read from the SMC over IOKit (no privileges needed). **macOS,
  Intel** — Apple Silicon uses different sensors, so it omits.
- **`top`** — the single hottest process right now and its CPU% (normalised to one core, so a
  multithreaded hog can exceed 100%).
- **power** — `ac` (with adapter watts) or `bat NN%`. Flags **`ac⚠ drain`** when you're on the
  wall but the battery is still discharging — the under-powered-adapter trap.

## Install

Requires **macOS**.

```sh
curl -fsSL https://raw.githubusercontent.com/Entrolution/machineline/main/install.sh | bash
```

The script downloads the right prebuilt binary (into `~/.local/bin`) and runs `machineline
install`, which merges a `statusLine` block into `~/.claude/settings.json` — backing it up first,
**capturing** whatever status line was already there so it can delegate to it, and refusing to
touch the file if it isn't valid JSON. Start a new session (or wait ~10s) and the line appears.

**From source** (needs a Rust toolchain):

```sh
git clone https://github.com/Entrolution/machineline.git
cd machineline
cargo build --release
./target/release/machineline install
```

To remove it (restores the captured status line verbatim, backs up first):

```sh
machineline uninstall          # or `--purge` to also delete the cache
```

Check what it reads, without installing:

```sh
machineline check
```

## Composition

machineline delegates rather than wraps. On install it records the existing `statusLine` command
to `~/.config/machineline/base.json`, then points `statusLine` at itself; at render time it runs
the captured command (forwarding Claude Code's stdin) and prints its output above its own line.
Installed alongside the others you get, top to bottom:

```
quota  …          ← quotaline (account usage)
vast   …          ← vastline (vast.ai GPU spend)
mach   …          ← machineline (this machine)
```

`uninstall` restores the captured command verbatim, so the chain unwinds cleanly.

## Configuration

`refreshInterval` (seconds, in the `statusLine` block) is how often the line re-renders when idle;
set it at install with `machineline install --refresh N`.

Environment overrides:

- `MACHINELINE_STATE_DIR` — where the cached snapshot lives (default `~/.claude/machineline`).
- `MACHINELINE_CONFIG_DIR` — where the captured base command lives (default `~/.config/machineline`).
- `CLAUDE_SETTINGS` — which settings file `install`/`uninstall` edit.

Colour thresholds are compile-time constants at the top of the `src` modules — change them and
rebuild.

## How it works

Claude Code runs the `statusLine` command on each render and pipes a JSON session payload on
stdin. machineline forwards that to the delegated base command, then reads its cached snapshot
(`~/.claude/machineline/state.json`) and prints — *immediately*. If the snapshot is older than a
few seconds it spawns a detached `machineline refresh` to update it for next time, so a render
never waits on a process spawn. The OS is read via libSystem (mach `host_statistics`, libc
`getloadavg`), the IOKit SMC (temperature), and a few stock CLIs (`pmset`, `vm_stat`, `ps`,
`sysctl`) — all only in the background `refresh`. CPU utilisation is the delta between two
refreshes' tick counters, so no sample ever sleeps the prompt.

## Platforms

Built for **macOS** (Apple Silicon + Intel). The throttle (`clk`) and temperature (`°C`) segments
are Intel-specific and disappear on Apple Silicon; load, utilisation, memory, swap, top-process
and power work on both. Pure Rust standard library plus `serde` — no system dependencies beyond
the stock command-line tools it shells out to.

## Notes & limits

- `clk` and `°C` rely on Intel-era `pmset`/SMC behaviour. On Apple Silicon they simply omit rather
  than guess.
- `top`'s `%cpu` comes from `ps` and is normalised to one core, so a busy multithreaded process can
  read above 100%.
- `~/.claude/machineline/` holds the cached snapshot. Safe to delete; it just re-gathers.

## License

[MIT](LICENSE)

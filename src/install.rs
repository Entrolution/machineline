//! `machineline install` / `uninstall`: wire machineline into `~/.claude/settings.json` as the
//! `statusLine` command, *capturing* whatever command was there before (e.g. vastline/quotaline)
//! so we can delegate to it at render time and restore it verbatim on uninstall.
//!
//! Composition model — delegation, not a shell wrapper:
//!   install:   save the existing statusLine block → config/base.json, then point statusLine at
//!              machineline. At render time machineline runs the saved command (stdin piped
//!              through) and prints its output below — i.e. above its own line.
//!   uninstall: restore the saved block verbatim (or remove statusLine if there was none); with
//!              `--purge`, also delete the captured base and the cache dir.
//!
//! Settings handling (backup-first, atomic, never clobber non-JSON) is ported from vastline.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Number, Value};

use crate::config::{config_dir, home_dir, state_dir};

fn settings_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("CLAUDE_SETTINGS") {
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    home_dir().map(|h| h.join(".claude").join("settings.json"))
}

fn base_path() -> PathBuf {
    config_dir().join("base.json")
}

/// The captured base status-line command we delegate to, if any. Read by the renderer.
pub fn base_command() -> Option<String> {
    let text = fs::read_to_string(base_path()).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let cmd = v.get("command").and_then(|c| c.as_str())?.trim();
    if cmd.is_empty() {
        None
    } else {
        Some(cmd.to_string())
    }
}

/// Quote the binary path for the shell Claude Code runs the command in. Ported from vastline.
fn quote_cmd(path: &str) -> String {
    if path.chars().any(char::is_whitespace) {
        format!("\"{}\"", path.replace('"', "\\\""))
    } else {
        path.to_string()
    }
}

fn backup(settings: &Path) -> std::io::Result<()> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let name = settings
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "settings.json".to_string());
    let bak = settings.with_file_name(format!("{name}.bak.{ts}"));
    fs::copy(settings, &bak)?;
    println!("backed up settings → {}", bak.display());
    Ok(())
}

fn write_atomic(path: &Path, v: &Value) -> std::io::Result<()> {
    let mut s = serde_json::to_string_pretty(v).unwrap_or_default();
    s.push('\n');
    let tmp = path.with_extension(format!("json.tmp.{}", std::process::id()));
    fs::write(&tmp, s)?;
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// Load settings as a JSON object, or `Err`. Missing/empty → empty object. Never overwrite a
/// non-object or invalid-JSON file.
fn load_object(settings: &Path) -> Result<Value, String> {
    if !settings.exists() {
        return Ok(Value::Object(Map::new()));
    }
    let text = fs::read_to_string(settings)
        .map_err(|e| format!("cannot read {}: {e}", settings.display()))?;
    if text.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    match serde_json::from_str::<Value>(&text) {
        Ok(v @ Value::Object(_)) => Ok(v),
        Ok(_) => Err(format!(
            "{} is not a JSON object — refusing to overwrite",
            settings.display()
        )),
        Err(_) => Err(format!(
            "{} is not valid JSON — refusing to overwrite (fix or move it, then re-run)",
            settings.display()
        )),
    }
}

/// Basename of the executable referenced by a statusLine command string. Handles a bare path, a
/// path with trailing arguments, and a double-quoted path (which `quote_cmd` produces for spaces).
fn command_basename(command: &str) -> Option<String> {
    let t = command.trim();
    let token = if let Some(rest) = t.strip_prefix('"') {
        rest.split('"').next().unwrap_or(rest)
    } else {
        t.split_whitespace().next().unwrap_or(t)
    };
    Path::new(token)
        .file_name()
        .and_then(|n| n.to_str())
        .map(String::from)
}

/// Does this statusLine command point at our own binary? Matches the exact current exe path or the
/// same basename, so a reinstall from a moved/renamed dir still recognises an old machineline
/// command as self instead of poisoning `base.json` with a self-reference.
fn command_is_self(command: &str, exe: &str) -> bool {
    if command.contains(exe) {
        return true;
    }
    match (
        command_basename(command),
        Path::new(exe).file_name().and_then(|n| n.to_str()),
    ) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// `command_is_self` against the *current* executable. Used by the renderer's loop-breaker.
pub fn looks_like_self(command: &str) -> bool {
    match std::env::current_exe() {
        Ok(p) => command_is_self(command, &p.to_string_lossy()),
        Err(_) => false,
    }
}

fn save_base(block: Option<&Value>) -> std::io::Result<()> {
    let dir = config_dir();
    fs::create_dir_all(&dir)?;
    let v = block.cloned().unwrap_or(Value::Null);
    let mut s = serde_json::to_string_pretty(&v).unwrap_or_else(|_| "null".into());
    s.push('\n');
    fs::write(base_path(), s)
}

fn read_base_block() -> Option<Value> {
    let text = fs::read_to_string(base_path()).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    if v.is_null() {
        None
    } else {
        Some(v)
    }
}

pub fn install(refresh: u64) -> i32 {
    let exe = match std::env::current_exe() {
        Ok(p) => p.to_string_lossy().into_owned(),
        Err(e) => {
            eprintln!("error: cannot resolve machineline's own path: {e}");
            return 1;
        }
    };
    let settings = match settings_path() {
        Some(p) => p,
        None => {
            eprintln!("error: cannot locate ~/.claude/settings.json (no HOME)");
            return 1;
        }
    };
    let mut root = match load_object(&settings) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("error: {msg}");
            return 1;
        }
    };

    // Capture the existing statusLine as our delegate — unless it's already us (re-install), in
    // which case keep whatever base we captured the first time.
    let existing = root.get("statusLine").filter(|v| !v.is_null()).cloned();
    let existing_is_self = existing
        .as_ref()
        .and_then(|b| b.get("command"))
        .and_then(|c| c.as_str())
        .map(|c| command_is_self(c, &exe))
        .unwrap_or(false);

    if settings.exists() {
        if let Err(e) = backup(&settings) {
            eprintln!("error: could not back up settings, refusing to modify them: {e}");
            return 1;
        }
    } else if let Some(parent) = settings.parent() {
        let _ = fs::create_dir_all(parent);
    }

    if existing_is_self {
        println!("machineline is already the statusLine command; keeping the captured base.");
    } else if let Err(e) = save_base(existing.as_ref()) {
        eprintln!("error: could not record the existing status line: {e}");
        return 1;
    }

    match read_base_block()
        .and_then(|b| b.get("command").and_then(|c| c.as_str()).map(String::from))
    {
        Some(cmd) => println!("delegating to existing status line: {cmd}"),
        None => println!("no existing status line to delegate to — machineline runs standalone."),
    }

    let command = quote_cmd(&exe);
    let mut block = Map::new();
    block.insert("type".into(), Value::String("command".into()));
    block.insert("command".into(), Value::String(command.clone()));
    block.insert(
        "refreshInterval".into(),
        Value::Number(Number::from(refresh)),
    );
    root.as_object_mut()
        .unwrap()
        .insert("statusLine".into(), Value::Object(block));

    if let Err(e) = write_atomic(&settings, &root) {
        eprintln!("error: could not write {}: {e}", settings.display());
        return 1;
    }

    println!("statusLine installed → {}", settings.display());
    println!("  command: {command}");
    println!("  refreshInterval: {refresh}s");
    println!("Start a new Claude Code session (or wait ~{refresh}s) to see the machine line.");
    0
}

pub fn uninstall(purge: bool) -> i32 {
    let settings = match settings_path() {
        Some(p) => p,
        None => {
            eprintln!("error: cannot locate ~/.claude/settings.json (no HOME)");
            return 1;
        }
    };

    if settings.exists() {
        let mut root = match load_object(&settings) {
            Ok(v) => v,
            Err(msg) => {
                eprintln!("error: {msg}");
                return 1;
            }
        };
        if let Err(e) = backup(&settings) {
            eprintln!("error: could not back up settings, refusing to modify them: {e}");
            return 1;
        }

        // Only touch statusLine if it's still ours. If the user has since pointed it elsewhere by
        // hand, leave their choice alone and just drop our captured base.
        let obj = root.as_object_mut().unwrap();
        let current_cmd = obj
            .get("statusLine")
            .and_then(|b| b.get("command"))
            .and_then(|c| c.as_str())
            .map(String::from);
        let exe = std::env::current_exe()
            .ok()
            .map(|p| p.to_string_lossy().into_owned());
        let current_is_self = match (&current_cmd, &exe) {
            (Some(cmd), Some(e)) => command_is_self(cmd, e),
            (Some(_), None) => true,
            (None, _) => false,
        };

        if current_is_self {
            match read_base_block() {
                Some(block) => {
                    obj.insert("statusLine".into(), block);
                    println!("restored the previous status line.");
                }
                None => {
                    obj.shift_remove("statusLine");
                    println!("removed machineline's statusLine block.");
                }
            }
        } else {
            println!(
                "statusLine is no longer machineline (you changed it) — leaving it untouched."
            );
        }
        if let Err(e) = write_atomic(&settings, &root) {
            eprintln!("error: could not write {}: {e}", settings.display());
            return 1;
        }
    } else {
        println!("nothing to do: {} not found", settings.display());
    }

    let _ = fs::remove_file(base_path());

    if purge {
        let cache = state_dir();
        match fs::remove_dir_all(&cache) {
            Ok(()) => println!("removed cache → {}", cache.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => eprintln!("warning: could not remove {}: {e}", cache.display()),
        }
        let cfg = config_dir();
        if fs::read_dir(&cfg)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false)
        {
            let _ = fs::remove_dir(&cfg);
        }
        println!("purged machineline state.");
    } else {
        println!("kept the cache. Re-run with `--purge` to remove it too.");
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_detection() {
        let exe = "/home/me/.local/bin/machineline";
        assert!(command_is_self("/home/me/.local/bin/machineline", exe));
        assert!(command_is_self("\"/home/me/.local/bin/machineline\"", exe));
        assert!(!command_is_self("/usr/local/bin/vastline", exe));
        assert!(command_is_self("/opt/machineline", exe)); // moved install, same basename
        assert!(!command_is_self("\"/usr/local/bin/vastline\" --foo", exe));
    }

    #[test]
    fn basename_extraction() {
        assert_eq!(
            command_basename("/a/b/machineline").as_deref(),
            Some("machineline")
        );
        assert_eq!(
            command_basename("\"/a b/machineline\"").as_deref(),
            Some("machineline")
        );
        assert_eq!(
            command_basename("/usr/bin/vastline").as_deref(),
            Some("vastline")
        );
    }
}

//! Build-time setup for `md380_emu_ambed`:
//!
//! - Verifies the firmware files needed by `include_bytes!` are
//!   present.
//! - Captures the current git commit short hash into the
//!   `GIT_HASH` env var so `clap`'s `--version` can show it.

use std::path::Path;
use std::process::Command;

fn main() {
    require_firmware_files();
    embed_git_hash();
    println!("cargo:rerun-if-changed=build.rs");
}

fn require_firmware_files() {
    for name in ["firmware/D002.032.img", "firmware/d02032-core.img"] {
        if !Path::new(name).exists() {
            eprintln!(
                "\n\n  ERROR: required firmware file missing: {name}\n\n  \
                 See md380_emu_ambed/firmware/README.org, or run `just sync-firmware`\n  \
                 from the workspace root.\n\n"
            );
            std::process::exit(1);
        }
        println!("cargo:rerun-if-changed={name}");
    }
}

fn embed_git_hash() {
    let hash = git_short_hash().unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=GIT_HASH={hash}");

    // Re-run if HEAD moves (commit / checkout / branch switch).
    // build.rs runs in the crate dir, so the workspace .git is one up.
    if Path::new("../.git/HEAD").exists() {
        println!("cargo:rerun-if-changed=../.git/HEAD");
    }
    if Path::new("../.git/refs/heads").exists() {
        println!("cargo:rerun-if-changed=../.git/refs/heads");
    }
}

/// First try `git rev-parse --short HEAD` (works wherever git is on
/// PATH, including the host). If git isn't available — typical in
/// minimal cross-build containers — fall back to parsing the
/// `.git/HEAD` + refs files directly.
fn git_short_hash() -> Option<String> {
    if let Some(h) = try_git_command() {
        return Some(h);
    }
    read_hash_from_git_files()
}

fn try_git_command() -> Option<String> {
    let out = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?;
    let s = s.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

fn read_hash_from_git_files() -> Option<String> {
    let head = std::fs::read_to_string("../.git/HEAD").ok()?;
    let head = head.trim();

    if let Some(ref_path) = head.strip_prefix("ref: ") {
        // HEAD points at a branch ref. Try the loose ref file first,
        // then packed-refs.
        let loose = std::fs::read_to_string(format!("../.git/{ref_path}")).ok();
        if let Some(s) = loose {
            return Some(s.trim().chars().take(7).collect());
        }
        let packed = std::fs::read_to_string("../.git/packed-refs").ok()?;
        for line in packed.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('^') {
                continue;
            }
            if let Some((hash, name)) = line.split_once(' ') {
                if name == ref_path {
                    return Some(hash.chars().take(7).collect());
                }
            }
        }
        None
    } else {
        // Detached HEAD — HEAD contains the full sha1.
        Some(head.chars().take(7).collect())
    }
}

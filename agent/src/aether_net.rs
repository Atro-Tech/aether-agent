// Æther network interceptor.
//
// Detects the VM's network interface (spr0 on Sprites, eth0 elsewhere).
// Attaches TC clsact qdisc so eBPF programs can be loaded later.
// Creates the BPF pin directory for credential routing maps.
//
// The actual eBPF programs are loaded by Shimmer from outside.
// We just set up the attachment points.

use std::path::Path;
use slog::{info, warn, Logger};

const SPRITES_IFACE: &str = "spr0";
const FALLBACK_IFACE: &str = "eth0";
const BPF_PIN_DIR: &str = "/sys/fs/bpf/aether";

/// Set up the network interceptor. Non-fatal — if anything fails,
/// the agent still works, just without eBPF network interception.
pub fn setup(logger: &Logger) {
    let iface = match detect_interface() {
        Some(i) => i,
        None => {
            warn!(logger, "Æther net: no network interface found");
            return;
        }
    };

    info!(logger, "Æther net: detected interface {}", iface);

    // Create BPF pin directory
    if let Err(e) = std::fs::create_dir_all(BPF_PIN_DIR) {
        warn!(logger, "Æther net: failed to create BPF pin dir: {}", e);
    }

    // Attach clsact qdisc (idempotent)
    let output = std::process::Command::new("tc")
        .args(["qdisc", "add", "dev", &iface, "clsact"])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            info!(logger, "Æther net: clsact attached to {}", iface);
        }
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr);
            if err.contains("File exists") {
                info!(logger, "Æther net: clsact already on {}", iface);
            } else {
                warn!(logger, "Æther net: tc failed: {}", err.trim());
            }
        }
        Err(e) => {
            warn!(logger, "Æther net: tc not available: {}", e);
        }
    }
}

fn detect_interface() -> Option<String> {
    for name in [SPRITES_IFACE, FALLBACK_IFACE] {
        if Path::new(&format!("/sys/class/net/{name}")).exists() {
            return Some(name.to_string());
        }
    }

    // Try any non-lo interface
    let entries = std::fs::read_dir("/sys/class/net").ok()?;
    for entry in entries.flatten() {
        if let Ok(name) = entry.file_name().into_string() {
            if name != "lo" {
                return Some(name);
            }
        }
    }

    None
}

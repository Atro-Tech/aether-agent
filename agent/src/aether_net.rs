// Æther Agent network interceptor.
//
// Attaches eBPF TC programs to the network interface (spr0 on Sprites,
// eth0 on E2B/Firecracker). All traffic flowing in and out passes through
// our eBPF programs in kernel space.
//
// Shimmer configures the rules via ttrpc → the agent writes them to
// BPF maps → the eBPF programs read the maps and enforce.
//
// The agent sets up the plumbing. Shimmer decides what to do with traffic.

use std::path::Path;

use anyhow::{Context, Result};
use slog::{info, warn, Logger};

/// Well-known interface names by platform.
const SPRITES_IFACE: &str = "spr0";
const FIRECRACKER_IFACE: &str = "eth0";

/// BPF pinned map paths — Shimmer writes rules here via ttrpc,
/// the eBPF programs read them in kernel space.
const BPF_PIN_DIR: &str = "/sys/fs/bpf/aether";
const CREDENTIAL_MAP: &str = "/sys/fs/bpf/aether/credentials";
const ROUTE_MAP: &str = "/sys/fs/bpf/aether/routes";

/// Detect which network interface we're on.
pub fn detect_interface() -> Option<String> {
    if Path::new(&format!("/sys/class/net/{SPRITES_IFACE}")).exists() {
        return Some(SPRITES_IFACE.to_string());
    }

    if Path::new(&format!("/sys/class/net/{FIRECRACKER_IFACE}")).exists() {
        return Some(FIRECRACKER_IFACE.to_string());
    }

    // Try to find any non-lo interface
    let entries = std::fs::read_dir("/sys/class/net").ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().into_string().ok()?;
        if name != "lo" {
            return Some(name);
        }
    }

    None
}

/// Set up the network interceptor.
/// Creates BPF pin directory and attaches TC clsact qdisc to the interface.
/// The actual eBPF programs are loaded separately.
pub fn setup(logger: &Logger) -> Result<()> {
    let iface = match detect_interface() {
        Some(i) => i,
        None => {
            warn!(logger, "Æther net: no network interface found, skipping");
            return Ok(());
        }
    };

    info!(logger, "Æther net: detected interface {}", iface);

    // Create BPF pin directory for maps
    std::fs::create_dir_all(BPF_PIN_DIR)
        .context("failed to create BPF pin directory")?;

    // Attach clsact qdisc to the interface (needed for TC BPF programs).
    // This is idempotent — if it already exists, tc returns an error we ignore.
    let output = std::process::Command::new("tc")
        .args(["qdisc", "add", "dev", &iface, "clsact"])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            info!(logger, "Æther net: attached clsact qdisc to {}", iface);
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            if stderr.contains("File exists") {
                info!(logger, "Æther net: clsact already on {}", iface);
            } else {
                warn!(logger, "Æther net: tc clsact failed: {}", stderr.trim());
            }
        }
        Err(e) => {
            warn!(logger, "Æther net: tc not available: {}", e);
        }
    }

    info!(logger, "Æther net: ready for eBPF programs on {}", iface);
    Ok(())
}

/// Write a credential routing rule to the BPF staging area.
/// Shimmer calls this via ttrpc. The eBPF TC program reads from these maps.
pub fn set_credential_route(
    match_pattern: &str,
    credential_key: &str,
    target_address: &str,
) -> Result<()> {
    std::fs::create_dir_all(BPF_PIN_DIR)?;

    // Append to the credential routes file.
    // Format: pattern|key|target per line.
    let entry = format!("{match_pattern}|{credential_key}|{target_address}\n");

    let path = Path::new(CREDENTIAL_MAP);
    let mut content = std::fs::read_to_string(path).unwrap_or_default();
    content.push_str(&entry);
    std::fs::write(path, content)?;

    Ok(())
}

/// Clear all credential routes.
pub fn clear_credential_routes() -> Result<()> {
    let path = Path::new(CREDENTIAL_MAP);
    if path.exists() {
        std::fs::write(path, "")?;
    }
    Ok(())
}

/// Get the interface name (for external callers).
pub fn interface_name() -> Option<String> {
    detect_interface()
}

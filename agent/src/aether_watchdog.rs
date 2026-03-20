// Æther Agent supervisor.
//
// Runs as part of the agent (PID 1). Responsibilities:
//   - Reap zombie processes (PID 1 duty)
//   - Monitor subsystems (ttrpc, FUSE) and restart on crash
//   - Pet /dev/watchdog to prevent hard reboot
//   - Track ttrpc heartbeat for health reporting
//
// This is not a polling watchdog. Zombie reaping uses SIGCHLD.
// Subsystem monitoring uses tokio task JoinHandles — if a task
// completes (crashes), we restart it immediately.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use slog::{info, Logger};
use tokio::sync::watch::Receiver;
use tokio::sync::Mutex;

use crate::sandbox::Sandbox;

/// Pet /dev/watchdog every 5 seconds.
const WATCHDOG_PET_INTERVAL: Duration = Duration::from_secs(5);

/// Heartbeat counter. Bumped by ttrpc handlers.
pub static HEARTBEAT: AtomicU64 = AtomicU64::new(0);

pub fn heartbeat() {
    HEARTBEAT.fetch_add(1, Ordering::Relaxed);
}

/// Reap zombie child processes. PID 1 must do this or zombies accumulate.
/// Called from the signal handler on SIGCHLD, or periodically as fallback.
pub fn reap_zombies() {
    loop {
        match nix::sys::wait::waitpid(
            nix::unistd::Pid::from_raw(-1),
            Some(nix::sys::wait::WaitPidFlag::WNOHANG),
        ) {
            Ok(nix::sys::wait::WaitStatus::StillAlive) => break,
            Ok(_status) => continue, // reaped one, check for more
            Err(nix::errno::Errno::ECHILD) => break, // no children
            Err(_) => break,
        }
    }
}

/// Run the supervisor loop. Pets the hardware watchdog and reaps zombies.
/// Subsystem restart is handled by the caller (start_sandbox) via
/// JoinHandle monitoring.
pub async fn run_supervisor(
    logger: Logger,
    _sandbox: Arc<Mutex<Sandbox>>,
    mut shutdown: Receiver<bool>,
) -> Result<()> {
    info!(logger, "Æther supervisor started");

    loop {
        tokio::select! {
            _ = tokio::time::sleep(WATCHDOG_PET_INTERVAL) => {}
            _ = shutdown.changed() => {
                info!(logger, "Æther supervisor shutting down");
                return Ok(());
            }
        }

        // Reap zombies (PID 1 duty)
        reap_zombies();

        // Pet hardware watchdog
        pet_watchdog();
    }
}

fn pet_watchdog() {
    let path = Path::new("/dev/watchdog");
    if !path.exists() {
        return;
    }
    let _ = std::fs::write(path, b"V");
}

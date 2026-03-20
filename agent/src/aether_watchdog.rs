// Æther Agent supervisor.
//
// We're a daemon, not PID 1. Sprites has its own init.
// Our job: keep the ttrpc server alive, track health, pet watchdog.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use slog::{info, Logger};
use tokio::sync::watch::Receiver;
use tokio::sync::Mutex;

use crate::sandbox::Sandbox;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

/// Heartbeat counter. Bumped by ttrpc handlers.
pub static HEARTBEAT: AtomicU64 = AtomicU64::new(0);

pub fn heartbeat() {
    HEARTBEAT.fetch_add(1, Ordering::Relaxed);
}

/// Supervisor loop. Pets /dev/watchdog if it exists.
pub async fn run_supervisor(
    logger: Logger,
    _sandbox: Arc<Mutex<Sandbox>>,
    mut shutdown: Receiver<bool>,
) -> Result<()> {
    info!(logger, "Æther supervisor started");

    loop {
        tokio::select! {
            _ = tokio::time::sleep(HEARTBEAT_INTERVAL) => {}
            _ = shutdown.changed() => {
                info!(logger, "Æther supervisor shutting down");
                return Ok(());
            }
        }

        // Pet hardware watchdog if it exists
        let path = Path::new("/dev/watchdog");
        if path.exists() {
            let _ = std::fs::write(path, b"V");
        }
    }
}

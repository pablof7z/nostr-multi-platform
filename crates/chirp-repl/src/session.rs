use std::time::Duration;

use crate::app::AppRuntime;

#[derive(Debug, Clone)]
pub struct LastRun {
    pub label: String,
    pub relays: usize,
    pub events: usize,
    pub new_events: usize,
}

pub struct Session {
    pub pubkey_hex: Option<String>,
    pub relays: Vec<String>,
    pub indexers: Vec<String>,
    pub last_run: Option<LastRun>,
    pub wall: Duration,
    pub app: AppRuntime,
}

impl Default for Session {
    fn default() -> Self {
        let bootstrap = nmp_core::chirp_default_relay_bootstrap();
        let relays: Vec<String> = bootstrap
            .iter()
            .filter(|entry| entry.role.contains("both"))
            .map(|entry| entry.url.to_string())
            .collect();
        let indexers: Vec<String> = bootstrap
            .iter()
            .filter(|entry| entry.role.contains("indexer"))
            .map(|entry| entry.url.to_string())
            .collect();
        let app = AppRuntime::new();
        for entry in bootstrap {
            let _ = app.add_relay(entry.url, entry.role);
        }
        Self {
            pubkey_hex: None,
            relays,
            indexers,
            last_run: None,
            wall: Duration::from_secs(8),
            app,
        }
    }
}

impl Session {
    pub fn active_pubkey(&self) -> crate::Result<&str> {
        self.pubkey_hex
            .as_deref()
            .ok_or_else(|| "no active identity - run load-key or create-account".to_string())
    }
}

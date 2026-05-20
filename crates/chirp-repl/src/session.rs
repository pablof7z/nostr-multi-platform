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
        let relays: Vec<String> = vec![
            "wss://relay.primal.net".to_string(),
            "wss://purplepag.es".to_string(),
        ];
        let indexers: Vec<String> = vec!["wss://purplepag.es".to_string()];
        let app = AppRuntime::new();
        for relay in &relays {
            let _ = app.add_relay(relay, "both");
        }
        for relay in &indexers {
            let _ = app.add_relay(relay, "indexer");
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

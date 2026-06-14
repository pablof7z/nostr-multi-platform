// D21 negative fixture — none of these lines may fire. They exercise every
// accepted shape: instance-field state (the K2 goal pattern), const data,
// read-once plain-config `OnceLock`s (the two benign residuals the rule is
// type-scoped to allow), `Mutex<()>` serialization tokens, and `#[cfg(test)]`
// test-double globals.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

// Accepted: a `const` is immutable — never ambient authority.
const MAX_IN_FLIGHT: usize = 8;

// Accepted: the K2 goal pattern — per-app state lives as an INSTANCE FIELD on a
// value the composition root threads by `Arc`-slot, not a process-global.
pub struct NmpApp {
    wallet_runtime: WalletRuntimeHandle,
    bunker_broker: Arc<BunkerBroker>,
}

// Accepted: read-once plain-config `OnceLock`. `bool` is `Copy` config, not a
// handle/runtime/sender/hook — this is the `wire_log.rs` ENABLED residual shape.
fn claim_log_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("NMP_CLAIM_LOG").is_some())
}

// Accepted: read-once plain-config `OnceLock<Option<PathBuf>>` — the
// `socket_io.rs` LOG_PATH residual shape. `PathBuf` is owned config, not authority.
fn log_path() -> Option<&'static PathBuf> {
    static LOG_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    LOG_PATH
        .get_or_init(|| std::env::var_os("NMP_WIRE_LOG").map(PathBuf::from))
        .as_ref()
}

// Accepted: a compiled-regex cache — `Regex` is plain derived config, not a
// handle that confers authority (the nmp-content regex_set.rs shape).
fn mention_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new("@[a-z]+").unwrap_or_default())
}

// Accepted: a read-once `String`/`&str` config value.
fn build_id() -> &'static str {
    static BUILD: OnceLock<String> = OnceLock::new();
    BUILD.get_or_init(|| "dev".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Accepted: `#[cfg(test)]` test doubles never run in production — a
    // process-global mock store or serialization lock here is fine.
    static SERIAL: Mutex<()> = Mutex::new(());
    static MOCK_STORE: OnceLock<Arc<BunkerBroker>> = OnceLock::new();

    #[test]
    fn t() {
        let _g = SERIAL.lock();
        let _ = MOCK_STORE.get_or_init(|| Arc::new(BunkerBroker));
    }
}

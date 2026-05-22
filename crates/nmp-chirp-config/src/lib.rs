//! Shared Chirp app configuration.
//!
//! This crate is intentionally dependency-free so platform-facing crates such
//! as `nmp-wasm` can share Chirp defaults without depending on `nmp-core`.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChirpRelayBootstrapEntry {
    pub url: &'static str,
    pub role: &'static str,
}

pub const CHIRP_CONTENT_RELAY_URL: &str = "wss://relay.primal.net";
pub const CHIRP_INDEXER_RELAY_URL: &str = "wss://purplepag.es";

pub const CHIRP_RELAY_BOOTSTRAP: &[ChirpRelayBootstrapEntry] = &[
    ChirpRelayBootstrapEntry {
        url: CHIRP_CONTENT_RELAY_URL,
        role: "both,indexer",
    },
    ChirpRelayBootstrapEntry {
        url: CHIRP_INDEXER_RELAY_URL,
        role: "indexer",
    },
];

pub fn chirp_default_relay_bootstrap() -> &'static [ChirpRelayBootstrapEntry] {
    CHIRP_RELAY_BOOTSTRAP
}

pub fn chirp_default_relay_urls() -> Vec<String> {
    CHIRP_RELAY_BOOTSTRAP
        .iter()
        .map(|entry| entry.url.to_string())
        .collect()
}

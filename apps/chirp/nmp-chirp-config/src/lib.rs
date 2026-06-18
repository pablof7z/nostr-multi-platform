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
        role: "both",
    },
    ChirpRelayBootstrapEntry {
        url: CHIRP_INDEXER_RELAY_URL,
        role: "indexer",
    },
];

#[must_use] 
pub fn chirp_default_relay_bootstrap() -> &'static [ChirpRelayBootstrapEntry] {
    CHIRP_RELAY_BOOTSTRAP
}

#[must_use]
pub fn chirp_default_relay_urls() -> Vec<String> {
    CHIRP_RELAY_BOOTSTRAP
        .iter()
        .map(|entry| entry.url.to_string())
        .collect()
}

/// Pubkeys (hex) every fresh Chirp account auto-follows out-of-the-box (kind:3).
///
/// This is Chirp PRODUCT policy, not NMP framework policy — NMP no longer
/// hardcodes any default follow set (#1493). The Chirp create-account FFI
/// wrapper (`nmp_app_chirp_create_new_account`) threads these into
/// `ActorCommand::CreateAccount { initial_follows, .. }`, the same Rust-owned
/// pattern the relay bootstrap uses — the seed pubkeys never transit the thin
/// native shell.
pub const CHIRP_DEFAULT_FOLLOWS: &[&str] = &[
    // npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft
    "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52",
    // fiatjaf
    "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d",
];

#[must_use]
pub fn chirp_default_follows() -> &'static [&'static str] {
    CHIRP_DEFAULT_FOLLOWS
}

//! Shared Chirp app configuration.
//!
//! This crate is intentionally dependency-free so platform-facing crates such
//! as `nmp-app-chirp`, Chirp TUI/desktop, and web codegen can share Chirp
//! defaults without depending on `nmp-core`.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChirpRelayBootstrapEntry {
    pub url: &'static str,
    pub role: &'static str,
}

pub const CHIRP_CONTENT_RELAY_URL: &str = "wss://relay.primal.net";
pub const CHIRP_INDEXER_RELAY_URL: &str = "wss://purplepag.es";
pub const CHIRP_SEARCH_RELAY_URL: &str = "wss://relay.nostr.band";
pub const CHIRP_PUBLIC_GROUP_RELAY_URL: &str = "wss://relay.groups.nip29.com";

/// Chirp's app-owned production bootstrap.
///
/// `CHIRP_CONTENT_RELAY_URL` is intentionally write-capable (`role:
/// "both,indexer"`) so a fresh browser session can prove publish acceptance
/// with a real terminal relay verdict while also retaining a connected
/// discovery lane. `purplepag.es` remains a pure discovery/index lane and must
/// not be counted as a write-proof target. These are Chirp operator choices,
/// not NMP framework defaults.
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

/// Chirp's app/operator default NIP-50 search relays.
///
/// User-authored kind:10007 search relays remain first authority; this list is
/// used only when the active account has not published one.
#[must_use]
pub fn chirp_default_search_relays() -> Vec<String> {
    vec![CHIRP_SEARCH_RELAY_URL.to_string()]
}

/// Chirp's suggested NIP-29 public-group host relay.
///
/// This is Chirp operator policy, surfaced through the generic NIP-29 defaults
/// projection so native shells do not hardcode the URL.
#[must_use]
pub fn chirp_public_group_relay_url() -> &'static str {
    CHIRP_PUBLIC_GROUP_RELAY_URL
}

/// Pubkeys (hex) every fresh Chirp account auto-follows out-of-the-box (kind:3).
///
/// This is Chirp PRODUCT policy, not NMP framework policy — NMP no longer
/// hardcodes any default follow set (#1493). The Chirp create-account FFI
/// wrapper (`nmp_app_chirp_create_new_account`) threads these into
/// `ActorCommand::Identity(IdentityCommand::CreateAccount { initial_follows, .. })`, the same Rust-owned
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

/// NIP-46 permission request Chirp advertises in client-initiated
/// `nostrconnect://` handshakes — the plain (NOT percent-encoded) comma-joined
/// perm list. This is Chirp PRODUCT policy, not NMP framework policy (#1493):
/// Chirp publishes kind:1 notes and kind:7 reactions, so it asks the remote
/// signer for exactly those two `sign_event` perms. NMP (the broker / core /
/// defaults) names no perm set of its own; only this leaf-app config does.
pub const CHIRP_NOSTRCONNECT_PERMS: &str = "sign_event:1,sign_event:7";

#[must_use]
pub fn chirp_nostrconnect_perms() -> &'static str {
    CHIRP_NOSTRCONNECT_PERMS
}

/// App-scoped keyring service id for the Marmot MLS DB encryption key.
///
/// Passed to `nmp_marmot_register_active` (and the Rust-internal
/// `register_with_secret_hex` sign-in path) as the
/// `keyring_service_id` parameter. Chirp-specific so other Marmot host apps
/// use their own namespace and never collide with Chirp's stored key (D0).
pub const CHIRP_MARMOT_KEYRING_SERVICE_ID: &str = "nmp.chirp.marmot";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_content_lane_is_write_capable_primal_indexer() {
        let content = CHIRP_RELAY_BOOTSTRAP
            .iter()
            .find(|entry| entry.url == CHIRP_CONTENT_RELAY_URL)
            .expect("content relay must be present in bootstrap");

        assert_eq!(CHIRP_CONTENT_RELAY_URL, "wss://relay.primal.net");
        assert_eq!(content.role, "both,indexer");
    }

    #[test]
    fn purplepages_remains_indexer_only() {
        let indexer = CHIRP_RELAY_BOOTSTRAP
            .iter()
            .find(|entry| entry.url == CHIRP_INDEXER_RELAY_URL)
            .expect("indexer relay must be present in bootstrap");

        assert_eq!(CHIRP_INDEXER_RELAY_URL, "wss://purplepag.es");
        assert_eq!(indexer.role, "indexer");
    }
}

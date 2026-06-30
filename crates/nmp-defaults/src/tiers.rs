//! The two composition tiers behind [`super::register_defaults`] — the
//! **substrate** tier (correctness) and the **social-feature** defaults tier —
//! plus the [`NmpDefaults`] config struct that parameterises them.
//!
//! # Why two tiers (the V-48 failure mode, restated)
//!
//! `register_defaults` historically fused two layers with *different
//! audiences*. **Substrate correctness** — without it the app is broken, not
//! minimal: routing returns `Unroutable`, `PublishTarget::Auto` fail-closes to
//! `NoTargets`, kind:10002 never populates the mailbox cache, oversized relay
//! plans are never trimmed. This is the `MinimalPlugins` analog from the Bevy
//! `DefaultPlugins` study — the irreducible floor every NMP app stands on.
//! **Social-feature defaults** — the nip02/nip17 action bundles, the
//! WOT/DM runtime controllers, the NIP-23 long-form typed projection — are
//! *preferences*, not correctness; a non-social external consumer
//! (podcast-player, hl, win-the-day) wants the floor without the ceiling.
//!
//! Before this split a non-social consumer had two bad options: call
//! `register_defaults` and swallow the social bundle, or hand-copy the
//! substrate block — which is *un-copyable* because it threads a single shared
//! `Arc<InMemoryMailboxCache>` through three seams (the mailbox-cache reader,
//! the routing factory, and the kind:10002 parser). Copy it wrong and the
//! writer (parser) and readers (router + NIP-19 encoder) desync. That is the
//! V-48 failure mode this crate exists to prevent — so the substrate tier is
//! now a *callable* [`register_substrate`], not a comment block.
//!
//! # The tier boundary (drawn here, honestly)
//!
//! **Substrate** ([`register_substrate`]): `nmp_router::register_actions` (the
//! `nmp.nip65.publish_relay_list` action — the routing crate's *own* action,
//! inseparable from the routing substrate it publishes for, NOT a social
//! toggle); the shared `Arc<InMemoryMailboxCache>` + mailbox-cache reader +
//! routing factory + kind:10002 parser (one cache, three clones); the
//! publish-resolver factory; the raw-event forward/republish policy; and the
//! `CoverageGate` coverage hook + NIP-77 negentropy runtime — the gate value
//! is **shared** between the hook and the runtime, so overriding it post-hoc
//! would desync them, which is why it is a [`NmpDefaults`] field, not a literal.
//!
//! **Social** (added by [`register_defaults`] on top): nip02/nip17 action
//! bundles, WOT/DM runtime controllers, the long-form typed projection, and
//! explicit app-supplied operator policy such as
//! `nostrconnect` and NIP-50 search fallback relays.
//!
//! # Implementation note
//!
//! `register_substrate` lives in `nmp-substrate-defaults` (the substrate-owning
//! crate) and is re-exported here for backward compatibility. Callers that want
//! only the substrate floor without this crate's social bundle can depend on
//! `nmp-substrate-defaults` directly.

use nmp_coverage_gate::CoverageGate;
use nmp_nip89::ClientIdentity;

use crate::SearchDefaults;

// Re-export the substrate floor from its canonical home.
pub use nmp_substrate_defaults::register_substrate;

/// Declarative configuration for [`super::register_defaults_with`] — the
/// config-as-fields pattern (Bevy's `.set(WindowPlugin { .. })` insight, and
/// the discoverability win from Spring Boot's configuration metadata: every
/// knob is a named, rustdoc'd field rather than a hardcoded literal buried in
/// the composition body).
///
/// [`NmpDefaults::default()`] is the no-operator-policy composition:
/// `register_defaults(app)` ≡ `register_defaults_with(app,
/// NmpDefaults::default())`, and leaf apps opt into relay-bearing policy by
/// filling the named fields before registration.
#[derive(Clone, Debug)]
pub struct NmpDefaults {
    /// Coverage policy shared by the D2 coverage hook **and** the NIP-77
    /// negentropy runtime. One value feeds *both* collaborators: the hook
    /// trims oversized relay plans to `max_relay_connections`, and the
    /// negentropy runtime reads the same gate to decide which large
    /// author×kind REQs to replace with NIP-77 sync. Overriding the gate
    /// post-hoc is impossible without desyncing them — which is precisely
    /// why it lives here as config rather than as a hardcoded
    /// `CoverageGate::default()` inside the substrate body.
    ///
    /// **Default:** [`CoverageGate::default()`] (`max_relay_connections = 30`).
    pub coverage_gate: CoverageGate,

    /// Fallback relay for client-initiated NIP-46 (`nostrconnect://`)
    /// handshakes when the app has no configured write relay. This is an
    /// operator-chosen relay URL — leaf-app policy, NOT an `nmp-defaults`
    /// default (#1493): NMP, including this composition library, owns no relay
    /// URLs. `None` means no fallback is wired; a `nostrconnect://` handshake
    /// then resolves the relay from the app's configured write relays and, if
    /// there are none, fails-closed (the FFI returns a null URI) rather than
    /// dialing any framework-chosen relay.
    ///
    /// A leaf app that wants a bootstrap fallback sets `Some(url)` here (or
    /// calls `AppHost::set_nostrconnect_bootstrap_relay` after
    /// `register_defaults`).
    ///
    /// **Default:** `None`.
    pub nostrconnect_bootstrap_relay: Option<String>,

    /// NIP-46 permission request advertised in client-initiated
    /// `nostrconnect://` handshakes — which event kinds the app asks the signer
    /// to sign (the plain, NOT percent-encoded, comma-joined NIP-46 perm list,
    /// e.g. `"sign_event:1,sign_event:7"`). This is leaf-app PRODUCT policy, NOT
    /// an `nmp-defaults` default (#1493): NMP, including this composition
    /// library, owns no perm set. `None` means no perms are wired and a
    /// `nostrconnect://` handshake omits the `&perms=` parameter entirely.
    ///
    /// A leaf app that wants to request perms sets `Some(perms)` here (or calls
    /// `AppHost::set_nostrconnect_perms` after `register_defaults`).
    ///
    /// **Default:** `None`.
    pub nostrconnect_perms: Option<String>,

    /// App-declared fallback search relays for NIP-50 when the active account
    /// has no user-authored kind:10007 search-relay list. This is operator
    /// policy, not framework policy: user kind:10007 relays remain first
    /// authority, this field is second, and an empty list means relay search is
    /// cache-only until the user publishes a list or the app supplies defaults.
    ///
    /// **Default:** empty.
    pub search_defaults: SearchDefaults,

    /// Wire the NIP-02 follow/unfollow/react action bundle **and** the WOT
    /// bootstrap runtime. The social graph layer. Disable for a non-social
    /// consumer that never follows, reacts, or computes web-of-trust.
    ///
    /// **Default:** `true`.
    pub social: bool,

    /// Wire the NIP-17 DM action bundle (`send` + `publish_relay_list`) **and**
    /// the DM-inbox runtime (kind:1059 gift-wrap inbox projection + relay-list
    /// reconciler). Disable for a consumer that never sends or receives DMs.
    ///
    /// **Default:** `true`.
    pub dms: bool,

    /// Wire the NIP-23 long-form (kind:30023) **typed** snapshot projection
    /// (`nmp.nip23.articles`, the `NL23` FlatBuffer). Disable for a consumer
    /// that never reads long-form articles.
    ///
    /// **Default:** `true`.
    pub longform: bool,

    /// App ClientIdentity declared once at the composition root. When `Some`,
    /// derives the relay User-Agent (always) and, if `attach_client_tag`, the
    /// NIP-89 `client` tag on PublicRoutable publishes.
    ///
    /// **Default:** `None` (transport falls back to the built-in `nmp/<ver>` UA;
    /// no client tag).
    pub client_identity: Option<ClientIdentity>,

    /// Opt-in: attach the NIP-89 public `client` tag to PublicRoutable publishes.
    /// Privacy default is OFF (the UA is always derived, but the public tag is
    /// opt-in). Ignored when `client_identity` is `None`.
    ///
    /// **Default:** `false`.
    pub attach_client_tag: bool,
}

impl Default for NmpDefaults {
    /// The canonical NMP wiring: `CoverageGate::default()`, every social
    /// feature on, and NO operator relay policy. Relay-bearing fields are empty
    /// or `None` — NMP ships no relay URL (#1493/#1924); a leaf app that wants
    /// a nostrconnect or search fallback supplies it explicitly.
    fn default() -> Self {
        Self {
            coverage_gate: CoverageGate::default(),
            nostrconnect_bootstrap_relay: None,
            nostrconnect_perms: None,
            search_defaults: SearchDefaults::default(),
            social: true,
            dms: true,
            longform: true,
            client_identity: None,
            attach_client_tag: false,
        }
    }
}

//! App-host registration seams.
//!
//! Reusable protocol and routing crates must not depend on the native C-ABI
//! crate just to wire their modules into an application. These traits live at
//! the substrate layer so crates can register actions, parsers, observers, and
//! runtime projections against any host that implements the same Rust contract.
//! `nmp-ffi::NmpApp` is one implementation, not the type every reusable crate
//! has to name.
//!
//! # D6 — narrow registration/capability traits
//!
//! The host surface is split into small, single-concern traits so a protocol
//! module receives ONLY the surface it actually uses (D0 capability honesty),
//! never a ~30-method god-object. A crate that only registers a parser takes
//! [`IngestParserRegistrar`]; a crate that only reacts to relay connects takes
//! [`RelayConnectedHookRegistrar`]; and so on. This mirrors the K2 capability
//! traits (`WalletKernelAccess`, `ProfileLookup`, …) — narrow contracts the
//! kernel hands out by least privilege.
//!
//! [`AppHost`] survives only as a **composition super-trait**: the union of
//! every narrow trait, implemented for free (blanket impl) by any type that
//! implements all of them. App/runtime composition roots are the one place that
//! genuinely need the whole surface and may name `AppHost`. Narrow consumers
//! must NOT.

use std::ops::Range;
use std::sync::Arc;

use crate::publish::OutboxResolver;
use crate::slots::{
    ActiveAccountSlot, ContactListReader, EventStoreSlot, IndexerRelaysSlot, LocalWriteRelaysSlot,
};
use crate::store::EventStore;
use crate::subs::PlanCoverageHook;
use crate::AppRelaySlot;

use super::{
    ActionRegistrar, DmInboxRelayLookup, ExternalEventSinkPolicy, IngestParser, MailboxCache,
    OutboxRouter, ProfileLookup, RawEventForwardPolicyContext, RelayConnectedHook,
    RelayTextInterceptor, ReqFrameInterceptor, RoutingTraceObserver,
};

mod observed;
mod projection;
pub use observed::{
    ObservedProjection, ObservedProjectionCommandHandle, ObservedProjectionRegistrar,
    ObservedProjectionSessionMap,
};
pub use projection::{IncrementalApplyError, SnapshotProjectionRegistrar};

/// Register ingest parsers (ADR-0070 / rule A5) — kind-keyed and range-keyed,
/// with slot-keyed lifecycle replace/unregister.
pub trait IngestParserRegistrar {
    fn register_ingest_parser(&self, kind: u32, parser: Arc<dyn IngestParser>);

    /// Slot-keyed replace: evict the prior parser registered under `slot_key`
    /// for `kind` (if any), then install `parser` under the same slot. Parsers
    /// registered under **other** slot keys (or via [`Self::register_ingest_parser`]
    /// with no slot key) are untouched.
    ///
    /// Used by lifecycle-managed singleton seams — each caller owns a unique
    /// `slot_key` (e.g. `"nip17.dm_inbox"` or `"marmot"`) and re-registrations
    /// only evict the caller's own prior entry. Multiple lifecycle-managed parsers
    /// on the same kind (e.g. the NIP-17 DM inbox and Marmot on kind:1059)
    /// coexist safely because they own distinct slots.
    ///
    /// Returns the previous parser for `(kind, slot_key)`, or `None` when this is
    /// the first registration for that slot. D6 — a poisoned dispatcher lock is a
    /// silent no-op returning `None` (the registration is dropped; existing parsers
    /// are preserved).
    ///
    /// **Slot keys MUST be globally unique across crates.** A second component
    /// reusing an existing slot name silently evicts the peer's parser. Choose a
    /// fully-qualified reverse-domain key (e.g. `"nip17.dm_inbox"`, `"marmot"`)
    /// that cannot collide with any other crate's registration.
    fn replace_ingest_parser(
        &self,
        kind: u32,
        slot_key: &'static str,
        parser: Arc<dyn IngestParser>,
    ) -> Option<Arc<dyn IngestParser>>;

    /// Remove the parser registered under `slot_key` for `kind`, if any.
    ///
    /// Used by teardown paths (e.g. Marmot sign-out without re-register) to
    /// clear a lifecycle-managed slot. D6 — a poisoned dispatcher lock is a
    /// silent no-op.
    fn unregister_ingest_parser(&self, kind: u32, slot_key: &'static str);

    /// Slot-keyed replace for a kind range: evict the prior range-parser
    /// registered under `slot_key` (if any), then install `parser` covering
    /// `range`. Parsers registered under other slot keys or via the slot-less
    /// [`Self::register_ingest_parser`] are untouched.
    ///
    /// Used by parsers that need to receive every kind (e.g. an all-kinds
    /// debug raw-event cache). Returns the previous parser for `slot_key`, or
    /// `None` when this is the first registration for that slot. D6 — a
    /// poisoned dispatcher lock is a silent no-op returning `None`.
    ///
    /// **Slot keys MUST be globally unique across crates.** Choose a
    /// fully-qualified reverse-domain key (e.g. `"chirp-tui.raw-cache"`) that
    /// cannot collide with any other crate's registration.
    fn replace_ingest_parser_range(
        &self,
        range: Range<u32>,
        slot_key: &'static str,
        parser: Arc<dyn IngestParser>,
    ) -> Option<Arc<dyn IngestParser>>;

    /// Remove the range-parser registered under `slot_key`, if any. D6 — a
    /// poisoned dispatcher lock is a silent no-op.
    fn unregister_ingest_parser_range(&self, slot_key: &'static str);
}

/// Register a Rust-side callback for active-account changes (per-account
/// lifecycle reset without polling).
pub trait IdentityChangeRegistrar {
    /// Register a Rust-side callback for active-account changes.
    ///
    /// The callback runs on the update-listener thread after the actor has
    /// written the active-keys slot and emitted an update frame. It
    /// fires only when the slot value changes (`Some(pubkey)` on sign-in /
    /// switch, `None` on logout / reset), never on ordinary snapshot ticks.
    /// This is the canonical composition seam for long-lived Rust objects that
    /// need to reset per-account state without polling.
    ///
    /// The callback receives the new active pubkey (hex), or `None` on
    /// logout / reset. No unregister is provided — current consumers are
    /// app-lifetime registrations installed during host init.
    ///
    /// This method lives on the trait — not only on the concrete `NmpApp` — so
    /// reusable protocol/runtime crates that register through `&impl
    /// IdentityChangeRegistrar` can wire per-account lifecycle hooks without
    /// depending on the C-ABI crate.
    fn register_identity_change_observer<F>(&self, f: F)
    where
        F: Fn(Option<String>) + Send + Sync + 'static;
}

/// Register a Rust-side callback for configured-relay changes.
pub trait ConfiguredRelaysChangeRegistrar {
    /// Register a callback that fires when the configured relay set changes.
    ///
    /// The callback receives no payload; consumers that need rows should read
    /// the shared [`AppRelaySlot`] returned by [`HostCapabilities`].
    fn register_configured_relays_change_observer<F>(&self, f: F)
    where
        F: Fn() + Send + Sync + 'static;
}

/// Install the host's `REQ`-frame interceptor (subscription-plan rewrite seam).
pub trait ReqFrameInterceptorRegistrar {
    fn set_req_frame_interceptor(&self, interceptor: Arc<dyn ReqFrameInterceptor>);
}

/// Install a relay-text interceptor (inbound relay message hook — e.g. the
/// NIP-47 wallet response listener).
pub trait RelayTextInterceptorRegistrar {
    fn add_relay_text_interceptor(&self, interceptor: Arc<dyn RelayTextInterceptor>);
}

/// Install a [`RelayConnectedHook`] so a protocol crate reacts when a relay
/// connects.
pub trait RelayConnectedHookRegistrar {
    /// ADR-0072 — install a [`RelayConnectedHook`] so a protocol crate (today
    /// `nmp-nip11`) reacts when a relay connects (e.g. fetch its NIP-11
    /// information document). Additive: multiple crates may react to the same
    /// connect.
    fn add_relay_connected_hook(&self, hook: Arc<dyn RelayConnectedHook>);
}

/// Install the subscription-plan coverage hook (planner diagnostics seam).
pub trait CoverageHookRegistrar {
    fn set_coverage_hook(&self, hook: PlanCoverageHook);
}

/// Install the kernel-owned enrichment readers (kind:0 profiles, kind:10002
/// mailbox hints) — the composition root passes the SAME `Arc` it backs the
/// matching [`IngestParser`] with, so reader and writer see one source of truth
/// (ADR-0070). The kernel never names the wire format (D0).
pub trait KernelReaderRegistrar {
    /// ADR-0070 PR 2 — install the kind:0 profile cache as the kernel's
    /// `Arc<dyn ProfileLookup>` (reader). The composition root passes the SAME
    /// `Arc` it backs the kind:0 [`IngestParser`] (`nmp_nip01::Kind0Parser`,
    /// the writer) with, so the kernel's enrichment / claim-TTL / zap-LNURL /
    /// RAM-eviction readers see one source of truth. The kernel never names the
    /// kind:0 wire format (D0).
    fn set_profile_lookup(&self, lookup: Arc<dyn ProfileLookup>);

    /// H4 — install the read-only [`MailboxCache`] handle the host's NIP-19
    /// identity encoder (UniFFI `encode_profile`) reads kind:10002 relay
    /// hints from. The composition root passes the SAME `MailboxCache`
    /// instance it hands [`RoutingFactoryRegistrar::set_routing_substrate`] and
    /// the kind:10002 [`IngestParser`], so the encoder can prefer `nprofile`
    /// over a bare `npub` using the hints the parser writes on ingest.
    /// Read-only, synchronous — no network, no actor round-trip.
    fn set_mailbox_cache_reader(&self, cache: Arc<dyn MailboxCache>);

    /// Install the protocol-owned contact-list reader.
    ///
    /// The NIP-02 crate owns the store scan and follow-tag parser; the kernel
    /// uses only this contact/follow fact for active-account recompile triggers
    /// and follow-edit safety gates.
    fn set_contact_list_reader(&self, reader: Arc<dyn ContactListReader>);
}

/// Install the DM-inbox relay-list lookup (NIP-17 kind:10050) — separate from
/// the other kernel readers because the DM protocol crate (`nmp-nip17`) is its
/// narrow consumer.
pub trait DmInboxRelayRegistrar {
    fn set_dm_inbox_relay_lookup(&self, lookup: Arc<dyn DmInboxRelayLookup>);
}

/// Install the blocked-relay lookup — the composition root passes the SAME
/// `Arc<InMemoryBlockedRelayCache>` (from `nmp-nip51`) it backs the
/// `Kind10006Parser` with, so the kernel's routing context always reads from
/// the same cache the parser writes.
///
/// Mirrors [`DmInboxRelayRegistrar`]: a separate narrow trait because this is
/// the router layer's concern, not the general kernel-reader seam.
pub trait BlockedRelayLookupRegistrar {
    fn set_blocked_relay_lookup(&self, lookup: Arc<dyn super::BlockedRelayLookup>);
}

/// Install the outbound routing / publish / raw-forward factories and the
/// NIP-46 bootstrap relay — the composition root's substrate-factory seam.
pub trait RoutingFactoryRegistrar {
    fn set_routing_substrate<F>(&self, factory: F)
    where
        F: Fn(Arc<dyn RoutingTraceObserver>) -> (Arc<dyn OutboxRouter>, Arc<dyn MailboxCache>)
            + Send
            + Sync
            + 'static;

    fn set_publish_resolver_factory<F>(&self, factory: F)
    where
        F: Fn(
                Arc<dyn EventStore>,
                Arc<dyn MailboxCache>,
                IndexerRelaysSlot,
                LocalWriteRelaysSlot,
                ActiveAccountSlot,
            ) -> Arc<dyn OutboxResolver>
            + Send
            + Sync
            + 'static;

    /// Register the external event sink policy factory.
    ///
    /// Policies returned by this factory receive typed [`SignedEventFrame`]s
    /// from the [`ExternalEventSinkDispatcher`] on a dedicated worker thread.
    /// Default no-op so AppHost impls that do not need event forwarding compile
    /// without changes; override in production composition roots.
    ///
    /// [`SignedEventFrame`]: nmp_signer_iface::SignedEventFrame
    /// [`ExternalEventSinkDispatcher`]: crate::substrate::ExternalEventSinkDispatcher
    fn set_external_event_sink_policy_factory<F>(&self, _factory: F)
    where
        F: Fn(RawEventForwardPolicyContext) -> Vec<Arc<dyn ExternalEventSinkPolicy>>
            + Send
            + Sync
            + 'static,
    {
    }

    /// Register the host-supplied fallback relay URL for client-initiated
    /// NIP-46 `nostrconnect://` handshakes.
    ///
    /// Must be called before `nmp_app_start`. The app/runtime composition root
    /// supplies the app's value; a per-app crate may override it. When no URL
    /// has been registered the
    /// substrate surfaces a typed error rather than silently using a hardcoded
    /// URL (V-65 / D0).
    fn set_nostrconnect_bootstrap_relay(&self, url: String);

    /// Register the host-supplied NIP-46 permission request advertised in
    /// client-initiated `nostrconnect://` handshakes.
    ///
    /// Must be called before `nmp_app_start`. `perms` is the comma-joined
    /// NIP-46 perm list in plain (NOT percent-encoded) form, e.g.
    /// `"sign_event:1,sign_event:7"`; the substrate percent-encodes it when it
    /// assembles the `&perms=` query parameter. Which event kinds an app asks
    /// the signer to sign is leaf-app product policy, not framework policy
    /// (#1493): NMP (including the composition library) supplies NO default, and
    /// when no perms are registered the handshake omits the `&perms=` parameter
    /// entirely rather than baking in a framework-chosen kind set.
    fn set_nostrconnect_perms(&self, perms: String);

    /// Register the relay-handshake User-Agent string derived from the app's
    /// ClientIdentity. `None`/unset → the transport's built-in `nmp/<ver>`
    /// fallback. Must be called before `nmp_app_start`. Composition-root wired
    /// (Flow A); leaf apps may override.
    fn set_relay_user_agent(&self, user_agent: String);

    /// Register substrate-generic outbound tag rows appended to PublicRoutable
    /// publishes (Flow B; the kernel names no NIP-89 noun — D0). Must be called
    /// before `nmp_app_start`. Default unset → no tags appended.
    fn set_outbound_public_tags(&self, tags: Vec<Vec<String>>);
}

/// Read-only host capability accessors — the active account identity, the
/// actor command channel, and the configured-relays slot.
///
/// D5 / D6: the raw `active_local_keys` accessor is intentionally NOT here.
/// Secret key material is reached only through the signer-session port /
/// `ProtocolCommandContext`, never the host registration surface — no host
/// consumer reads `nostr::Keys` through this trait. Identity-only consumers
/// (WOT bootstrap, DM relay-list runtime, zap-receipt / mute reconcilers) read
/// [`Self::active_pubkey`], which is populated for every backend including
/// remote-signer (NIP-46 bunker) accounts.
pub trait HostCapabilities {
    /// Pubkey-only identity accessor.
    ///
    /// Returns the shared [`ActiveAccountSlot`] (`Arc<Mutex<Option<String>>>`,
    /// hex pubkey) the kernel actor writes on every identity mutation. Unlike
    /// the raw-keys accessor — which is `None` for remote-signer (NIP-46
    /// bunker) accounts whose secret material lives outside the kernel — this
    /// slot is populated for **every** backend, including bunker. Identity-only
    /// consumers (WOT bootstrap, the DM relay-list runtime, self-zap-receipt and
    /// mute-list reconcilers) MUST read this so they activate for bunker
    /// accounts.
    ///
    /// Single source of truth (D4): this is the exact slot the actor populates
    /// in `kernel::identity_state` — it is not a second mirror of the active
    /// account. `None` means no account is signed in.
    fn active_pubkey(&self) -> ActiveAccountSlot;

    fn actor_sender(&self) -> crate::actor::CommandSender;

    fn configured_relays_handle(&self) -> AppRelaySlot;

    /// Clone the kernel-published [`EventStoreSlot`] when a reusable runtime
    /// needs cache-first replay from canonical storage.
    ///
    /// Default is an empty slot so scaffold/test hosts that only need active
    /// account + command access keep compiling. Real composition hosts override
    /// this with the actor-published slot.
    fn event_store_handle(&self) -> EventStoreSlot {
        crate::slots::new_event_store_slot()
    }

    /// Install a host-side preferred-relay provider (a `(primary, fallback)`
    /// relay-list pair the host resolves at use time, e.g. the active account's
    /// published list → an app default). The composition root wires this so a
    /// protocol crate that fans an interest to a per-account relay set can read
    /// the resolved relays through the host without naming the host type.
    ///
    /// Substrate-generic: the provider returns plain relay-URL lists — no NIP
    /// noun crosses this seam (same posture as [`super::BlockedRelayLookup`] /
    /// `DmInboxRelayLookup`, which are NIP-keyed lists named generically here).
    ///
    /// **Default is a no-op**, so a minimal / scaffolded host that implements
    /// `AppHost` compiles and runs for free without a relay provider — only a
    /// real composition host (`NmpApp`) overrides this to store the provider.
    /// The first consumer is NIP-50 search: `nmp-nip51` wires the kind:10007
    /// list and app-supplied `SearchFallbackRelays`; `nmp-nip50` reads the
    /// provider through this seam.
    fn install_preferred_relay_source(&self, _source: std::sync::Arc<dyn PreferredRelaySource>) {}
}

/// A host-installed provider of a `(primary, fallback)` relay-URL list pair,
/// resolved at use time. Substrate-generic: returns plain `wss://` URLs with no
/// NIP noun. Installed via [`HostCapabilities::install_preferred_relay_source`]
/// and read by a protocol crate (NIP-50 search) that needs the active account's
/// preferred relay set without naming the host type (D0).
pub trait PreferredRelaySource: Send + Sync {
    /// The primary relay list (e.g. the active account's published kind:10007
    /// search relays). Empty when none are known.
    fn primary(&self) -> Vec<String>;

    /// The fallback relay list (e.g. the app default) used when `primary()` is
    /// empty. Empty when the app declared none.
    fn fallback(&self) -> Vec<String>;
}

/// Host surface needed by reusable NMP **composition roots**.
///
/// D6: this is the union super-trait of every narrow registration / capability
/// trait above. It is implemented for free (blanket impl below) by any type
/// that implements all of them — app/runtime composition roots name it because
/// they genuinely wire the whole surface. Narrow protocol modules MUST take the
/// specific narrow trait(s) they use, never `AppHost`.
///
/// This is the shared composition target for host-backed NMP runtimes, not a
/// native shell trait. Native storage, OS keychains, browser WebSocket handles,
/// and other platform capabilities stay outside this surface. The methods here
/// register Rust-owned facts only: action modules, ingest parsers, snapshot
/// projections, observed projections, routing factories, capability seams, and
/// read-only kernel slots. A browser builder that implements the same narrow
/// registrars receives this trait through the blanket impl and can call the
/// same explicit installer path without exposing reducer internals.
pub trait AppHost:
    ActionRegistrar
    + ConfiguredRelaysChangeRegistrar
    + SnapshotProjectionRegistrar
    + IngestParserRegistrar
    + ObservedProjectionRegistrar
    + IdentityChangeRegistrar
    + ReqFrameInterceptorRegistrar
    + RelayTextInterceptorRegistrar
    + RelayConnectedHookRegistrar
    + CoverageHookRegistrar
    + KernelReaderRegistrar
    + DmInboxRelayRegistrar
    + BlockedRelayLookupRegistrar
    + RoutingFactoryRegistrar
    + super::search::SearchScopeRegistrar
    + super::intent::InputScopeRegistrar
    + HostCapabilities
{
}

impl<T> AppHost for T where
    T: ActionRegistrar
        + ConfiguredRelaysChangeRegistrar
        + SnapshotProjectionRegistrar
        + IngestParserRegistrar
        + ObservedProjectionRegistrar
        + IdentityChangeRegistrar
        + ReqFrameInterceptorRegistrar
        + RelayTextInterceptorRegistrar
        + RelayConnectedHookRegistrar
        + CoverageHookRegistrar
        + KernelReaderRegistrar
        + DmInboxRelayRegistrar
        + BlockedRelayLookupRegistrar
        + RoutingFactoryRegistrar
        + super::search::SearchScopeRegistrar
        + super::intent::InputScopeRegistrar
        + HostCapabilities
{
}

#[cfg(test)]
mod tests;

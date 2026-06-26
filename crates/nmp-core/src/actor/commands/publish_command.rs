//! `PublishCommand` — sign-and-publish + publish-engine control (ADR-0065).
//!
//! Grouped under `ActorCommand::Publish(PublishCommand)`. Dispatch home:
//! `actor/dispatch/cmd_publish.rs`.

/// Sign-and-publish verbs + publish-engine control (retry / cancel).
///
/// These are the *publish* paths — each signs (locally or via the ADR-0050
/// port) AND routes through the NIP-65 outbox / explicit relay set. The
/// ADR-0050 *sign-only* verbs live in [`super::super::SignCommand`].
#[derive(Debug)]
pub enum PublishCommand {
    /// Sign-and-publish an arbitrary event kind for the active account.
    /// The actor fills `pubkey` from the active signer, stamps `created_at`
    /// (D7), signs, and routes through the NIP-65 outbox per `target`.
    /// Dispatched by `PublishAction::PublishRaw` via `dispatch_action`.
    ///
    /// Both local-keys and remote (NIP-46) signer accounts are supported —
    /// the dispatch arm delegates to the existing `publish_unsigned_event` /
    /// `publish_unsigned_event_to_relays` helpers, which already park bunker
    /// signs as a `ParkedOp` with the `Publish` sink (D8 — actor never blocks).
    RawEvent {
        kind: u32,
        tags: Vec<Vec<String>>,
        content: String,
        target: crate::publish::PublishTarget,
        /// When `Some(pubkey)`, the actor signs with the account whose pubkey
        /// matches — looked up across BOTH local keys and remote signers —
        /// via `sign_with_account_nonblocking`, instead of the active account.
        /// This is the `PublishAction::PublishRaw` signer selector: it lets an
        /// agent / per-podcast key publish without ever becoming the active
        /// account. `None` preserves the legacy behaviour: sign with the active
        /// account.
        signer_pubkey: Option<String>,
        correlation_id: Option<String>,
    },
    /// Sign-and-publish a kind:1 reply to an event already present in the
    /// kernel store. The caller supplies only user intent (`content` +
    /// direct parent id); the headless/native reducer derives NIP-10
    /// root/reply/p tags from the stored parent before the sign boundary.
    Reply {
        content: String,
        reply_to_event_id: String,
        target: crate::publish::PublishTarget,
        signer_pubkey: Option<String>,
        correlation_id: Option<String>,
    },
    /// T66a publish — sign a kind:0 profile metadata event with the active
    /// account and emit it to the NIP-65 outbox-resolved write relays (D3).
    ///
    /// `fields` is the flat string map the host supplied; the actor serializes
    /// it into the kind:0 `content`, stamps `created_at` from
    /// `kernel.now_secs()` (the host never hand-rolls the timestamp), and
    /// signs. Sibling of [`Self::RawEvent`] — same sign-and-publish path,
    /// kind:0 instead of an arbitrary kind.
    ///
    /// `correlation_id` is the registry-minted action id when this command
    /// originates from `nmp_app_dispatch_action`
    /// (`PublishAction::PublishProfile`). Threading it through makes the
    /// publish engine report it in `action_results` so the host spinner keyed
    /// on the dispatch return value can be cleared. `None` for non-dispatch
    /// callers.
    Profile {
        fields: serde_json::Map<String, serde_json::Value>,
        correlation_id: Option<String>,
    },
    /// Generic, kind-agnostic publish — take an `UnsignedEvent` already built
    /// by any protocol-crate builder (`nmp_nip23::Article`, `nmp_nip01::Note`,
    /// `nmp_relations::Reaction`, …), sign with the active account's keys, and
    /// route through the NIP-65 outbox resolver (D3). The kernel does not
    /// inspect the kind — that's the protocol crate's concern (D0).
    ///
    /// Stepping stone toward per-protocol-crate `ActionModule` impls
    /// (`kind-wrappers.md` §8 Phase 1); deprecates kind-by-kind as those land.
    ///
    /// `correlation_id` is the registry-minted action id when this command
    /// originates from an `ActionModule::execute` call. Threading it lets the
    /// publish engine report THAT id in `action_results` (via
    /// `correlation_id_override`) so the host spinner closes on the id it
    /// received from `dispatch_action`, not on the signed event's id. `None`
    /// for callers that are not action-dispatched (e.g. direct `NmpApp::` Rust
    /// API calls, conformance tests).
    UnsignedEvent {
        event: nmp_signer_iface::UnsignedEvent,
        correlation_id: Option<String>,
        /// When `Some(pubkey)`, the actor signs with the account whose pubkey
        /// matches — looked up across BOTH local keys and remote signers —
        /// via `sign_with_account_nonblocking`, instead of the active account.
        /// This lets a non-active account publish without first switching
        /// active. `None` preserves the legacy behaviour (sign with the active
        /// account and fail closed when no account is active).
        signer_pubkey: Option<String>,
    },
    /// Publish an unsigned event to an explicit relay set, bypassing the
    /// NIP-65 outbox resolver. Used by action executors that target a specific
    /// relay pin (e.g. NIP-29 group relays). D4: only the actor signs and
    /// publishes. D8: no blocking — relay dispatch is async.
    ///
    /// Sibling to [`Self::UnsignedEvent`] (which routes via the NIP-65 outbox)
    /// and [`Self::SignedEvent`] (which carries an already-signed event). This
    /// variant SIGNS with the active account like the unsigned sibling, but
    /// ROUTES to exactly `relays` like the signed sibling's `Explicit` mode —
    /// the combination a host-pinned group action needs. A NIP-29 join request
    /// must reach the group's own host relay, never the author's kind:10002
    /// outbox.
    ///
    /// Like the unsigned sibling, the event's `pubkey` is derived from the
    /// active identity at sign time; the caller's `event.pubkey` is ignored.
    /// Empty or malformed `relays` fail closed in the publish handler.
    UnsignedEventToRelays {
        event: nmp_signer_iface::UnsignedEvent,
        relays: Vec<crate::publish::RelayUrl>,
        /// Registry-minted `correlation_id` from `dispatch_action`, when this
        /// command originates from an `ActionModule::execute` call. Threading
        /// it lets the publish engine report THAT id in `action_results` (via
        /// `correlation_id_override`) so the host spinner closes on the id it
        /// received from `dispatch_action`, not on the signed event's id.
        /// `None` for callers that are not action-dispatched.
        correlation_id: Option<String>,
        /// When `Some(pubkey)`, the actor signs with the account whose pubkey
        /// matches — looked up across BOTH local keys and remote signers —
        /// via `sign_with_account_nonblocking`, instead of the active account.
        /// `None` preserves the legacy behaviour (sign with the active account).
        signer_pubkey: Option<String>,
    },
    /// Generic publish of an **already-signed** event. The kernel verifies
    /// the Schnorr signature + event-id hash, then routes the event verbatim
    /// through the same planner / NIP-65 outbox / relay-pin path the unsigned
    /// command uses — the signer is never consulted (no re-signing). Unlike
    /// [`Self::UnsignedEvent`], this does not require an active account: the
    /// signature already exists and routing keys off the event's own pubkey.
    /// Generic capability (D0); externally-signed group events are the first
    /// consumer but the kernel has no protocol nouns.
    ///
    /// `target` selects the D3 routing mode without erasing intent: `Auto`
    /// asks the kernel to resolve via NIP-65, while `Explicit { relays }`
    /// dispatches to exactly those relays and fails closed when the set is
    /// empty or malformed.
    ///
    /// `correlation_id` is the registry-minted action id when this publish
    /// originates from `nmp_app_dispatch_action`'s `PublishAction::Publish`
    /// path. `None` for non-dispatch callers (`NmpApp::publish_signed_explicit`
    /// — Marmot's MLS / gift-wrap seam — and conformance harnesses); the
    /// engine then falls back to the publish handle (== event id), preserving
    /// prior behaviour. For the dispatched pre-signed `Publish` path this
    /// `correlation_id` is the registry-minted operation identity — never the
    /// event id (#1748).
    SignedEvent {
        raw: crate::store::RawEvent,
        target: crate::publish::PublishTarget,
        correlation_id: Option<String>,
    },
    /// User intent from the outbox UI: retry a still-pending publish now.
    RetryPublish { handle: String },
    /// User intent from the outbox UI: cancel a still-pending publish,
    /// addressed by the operation's `correlation_id` (S7, #1754). The kernel's
    /// cancel-by-id doorway reverse-resolves the publish handle from the
    /// durable handle↔correlation index and records the user-initiated
    /// `Cancelled` terminal under this ORIGINAL `correlation_id` (PD-036). A
    /// raw publish handle is also accepted (the index self-maps it) so
    /// internal callers that only know the handle still resolve.
    CancelPublish { correlation_id: String },
}

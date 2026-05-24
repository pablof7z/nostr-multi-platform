//! `nmp.nip65.publish_relay_list` — NIP-65 relay-list (kind:10002) publish path.
//!
//! Absorbed from the (now-deleted) `nmp-nip65` crate at step 3 of the
//! crate-boundary migration (`docs/architecture/crate-boundaries.md` §5).
//! `nmp-router` is the single home for both the kind:10002 ingest parser
//! ([`crate::Kind10002Parser`]) + cache ([`crate::InMemoryMailboxCache`])
//! **and** the kind:10002 publish action: routing owns the kind end-to-end.
//!
//! # Why this exists
//!
//! The kernel already INGESTS kind:10002 events (via
//! `Kind10002Parser`/`InMemoryMailboxCache`) to populate the live NIP-65
//! cache the [`crate::GenericOutboxRouter`] consults. That cache is what
//! every publish + REQ fan-out reads for "where does this author
//! read/write?".
//!
//! But the actor's local `AddRelay` / `RemoveRelay` arms only mutate the
//! `RelayEditRow` projection and dial / drop sockets — they never publish
//! a new kind:10002 that reflects the change. The result is asymmetric:
//!
//! * a user removes a defunct relay → no kind:10002 update → other clients
//!   still fan REQs and publishes out to a dead host;
//! * a user adds a new relay → never advertised → contacts have no signal
//!   to read or write there.
//!
//! `nmp.nip65.publish_relay_list` closes that loop: a host (or the actor's
//! own AddRelay/RemoveRelay arms, via a sibling in-tree helper) publishes
//! a kind:10002 reflecting the user's intended relay set. The kernel then
//! ingests its own publish exactly as any other client's, keeping the
//! NIP-65 cache for the active account in sync with the `RelayEditRow`
//! projection without a special case.
//!
//! # Tag shape — NIP-65
//!
//! kind:10002 carries `["r", <wss-url>]` tags. The optional third element
//! is the role marker:
//!
//! * `["r", <url>]`           → read + write (default, parsed as "both")
//! * `["r", <url>, "read"]`   → read-only
//! * `["r", <url>, "write"]`  → write-only
//!
//! Any third-element value other than `"read"` / `"write"` is parsed by
//! the kernel as "both" (see `nmp-core::kernel::nostr::parse_relay_list`:
//! `let marker = tag.get(2).map(String::as_str).unwrap_or("both")`). The
//! builder here MUST agree with that parser so a publish → ingest round
//! trip is lossless.
//!
//! # Routing
//!
//! kind:10002 is itself a NIP-65 replaceable event (`10000 ≤ kind < 20000`).
//! The executor enqueues [`ActorCommand::PublishUnsignedEvent`] — the
//! kernel's Auto path — so the very first kind:10002 for a freshly-created
//! account hits the bootstrap discovery relays (no chicken-and-egg), and
//! later updates land on the author's own write set.
//!
//! # D7 — `created_at` sentinel
//!
//! The unsigned event is built with `created_at: 0`. The actor re-stamps
//! it from `kernel.now_secs()` before signing (see the
//! `PublishUnsignedEvent` arm in `nmp-core::actor::dispatch`); this module
//! never reads the system clock.
//!
//! # D0 — namespace
//!
//! The action namespace is `nmp.nip65.publish_relay_list` — byte-stable
//! across the move from `nmp-nip65` so callers do not need to change.

use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection, UnsignedEvent};
use nmp_core::{canonical_relay_url, ActorCommand};
use serde::{Deserialize, Serialize};

/// NIP-65 kind: the relay list — read/write outbox/inbox advertisement.
const KIND_RELAY_LIST: u32 = 10002;

/// Per-relay role marker for a NIP-65 entry.
///
/// The wire format on kind:10002 is:
/// * [`Both`] → tag `["r", url]` with no third element (the default).
/// * [`Read`] → tag `["r", url, "read"]`.
/// * [`Write`] → tag `["r", url, "write"]`.
///
/// The kernel parser treats *any* third-element string other than `"read"`
/// or `"write"` as "both", but to keep the publish → ingest round-trip
/// stable in the canonical case the builder OMITS the third element for
/// [`Both`] rather than emitting `"both"`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RelayMarker {
    /// Read + write. Wire form: `["r", url]` (no marker).
    #[default]
    Both,
    /// Read-only. Wire form: `["r", url, "read"]`.
    Read,
    /// Write-only. Wire form: `["r", url, "write"]`.
    Write,
}

/// One relay entry in the user's NIP-65 outbox/inbox advertisement.
///
/// `url` is canonicalised by the builder (lowercase scheme+host, trailing
/// `/` stripped on empty path). Non-`wss://` / `ws://` URLs are dropped —
/// the kernel's ingest parser requires `wss://`, and the builder mirrors
/// that gate so a publish → ingest round-trip is stable.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct RelayListEntry {
    /// Relay URL. Canonicalised before being written to the tag.
    pub url: String,
    /// Read/write role marker. Defaults to [`RelayMarker::Both`].
    #[serde(default)]
    pub marker: RelayMarker,
}

/// Build a NIP-65 kind:10002 relay-list **unsigned** event from an explicit
/// list of [`RelayListEntry`] values.
///
/// Per NIP-65, each entry becomes an `["r", <url>]` tag with an optional
/// `"read"` / `"write"` third element. The default marker [`RelayMarker::Both`]
/// omits the third element entirely (matching the kernel parser's
/// `.unwrap_or("both")` branch); the explicit `"read"` / `"write"` markers
/// emit the marker verbatim.
///
/// URLs are canonicalised via [`nmp_core::canonical_relay_url`] (lowercase
/// scheme+host, trailing-`/` stripped on empty path) and deduplicated by
/// canonical URL in first-seen order. URLs that do not parse as `ws://` or
/// `wss://` are dropped — this matches the ingest parser's `wss://` gate so a
/// build → ingest round-trip is stable. (`ws://` is accepted by the
/// canonicaliser but will be SKIPPED by the kernel parser, which requires
/// `wss://`; callers should configure `wss://`.)
///
/// Dedup is by canonical URL only — two entries for the same host with
/// different markers collapse to the *first* marker seen. Callers that
/// need to express "both directions" should set [`RelayMarker::Both`]
/// once; emitting two tags (one `read`, one `write`) for the same host is
/// not what NIP-65 specifies and the kernel parser would not re-merge
/// them correctly.
///
/// The returned event:
/// * has `kind = 10002`,
/// * has `created_at = 0` — the D7 sentinel; the actor re-stamps it,
/// * has an empty `pubkey` — the actor derives it from the signing keys at
///   sign time (this mirrors `nmp_nip17::build_dm_relay_list_event` and the
///   NIP-29 builders; the build half is pubkey-agnostic).
#[must_use]
pub fn build_relay_list_event(entries: &[RelayListEntry]) -> UnsignedEvent {
    let mut tags: Vec<Vec<String>> = Vec::with_capacity(entries.len());
    let mut seen = std::collections::HashSet::new();
    for entry in entries {
        let Some(canonical) = canonical_relay_url(&entry.url) else {
            continue;
        };
        if !seen.insert(canonical.clone()) {
            continue;
        }
        let tag = match entry.marker {
            RelayMarker::Both => vec!["r".to_string(), canonical],
            RelayMarker::Read => vec!["r".to_string(), canonical, "read".to_string()],
            RelayMarker::Write => vec!["r".to_string(), canonical, "write".to_string()],
        };
        tags.push(tag);
    }
    UnsignedEvent {
        // Empty placeholder — the actor re-derives the pubkey from the
        // signing key at sign time (see `ActorCommand::PublishUnsignedEvent`).
        pubkey: String::new(),
        kind: KIND_RELAY_LIST,
        tags,
        content: String::new(),
        // D7 sentinel — the actor re-stamps from `kernel.now_secs()`.
        created_at: 0,
    }
}

/// Wire shape for `nmp.nip65.publish_relay_list` — the JSON a host passes to
/// `nmp_app_dispatch_action`.
///
/// `relays` is the user's full NIP-65 relay set. The host is the source of
/// truth here: it reads the user's configured relays from its own UI state
/// (typically the same `RelayEditRow` projection the kernel exposes) and
/// hands them in. Keeping the action stateless (no kernel reads in the
/// executor) is consistent with the rest of the action surface — the
/// executor closure receives only the JSON, the correlation id, and a send
/// callback.
///
/// The auto-trigger path from `actor::dispatch::AddRelay` / `RemoveRelay`
/// is sibling to this action, NOT a caller of it: the actor reads its own
/// `RelayEditRow` projection and calls `build_relay_list_event` directly,
/// because `ActionContext` does not carry kernel state and `execute`'s
/// signature is `(action, correlation_id, send)`. Both paths converge on
/// the same on-wire kind:10002 shape via the shared builder above.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PublishRelayListInput {
    /// Relays to advertise as the user's NIP-65 relay set. Canonicalised
    /// and deduped by the builder; URLs that do not parse as `wss://` /
    /// `ws://` are dropped.
    pub relays: Vec<RelayListEntry>,
}

/// The `nmp.nip65.publish_relay_list` [`ActionModule`] — a pure shape validator.
///
/// Mirrors `nmp_nip17::PublishDmRelayListAction`'s discipline: `start` is
/// a side-effect-free shape check; the actual sign + publish happens on
/// the actor thread (D7) via `ActorCommand::PublishUnsignedEvent`.
pub struct PublishRelayListAction;

impl ActionModule for PublishRelayListAction {
    const NAMESPACE: &'static str = "nmp.nip65.publish_relay_list";
    type Action = PublishRelayListInput;

    /// Reject an empty relay set — a kind:10002 with zero `r` tags is the
    /// canonical "I cleared my NIP-65 metadata" signal in
    /// `ingest_relay_list` (`nmp-core::kernel::ingest::relay_list`), which
    /// REMOVES the cache entry and forces every subsequent fan-out for
    /// this author through the cold-start bootstrap discovery seed. That
    /// is a destructive operation and should not be reachable via the
    /// "publish my list" verb. A host wanting to explicitly clear the
    /// list needs its own explicit verb (this v1 does not ship one).
    fn start(_ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        if action.relays.is_empty() {
            return Err(ActionRejection::Invalid(
                "empty NIP-65 relay list — refusing to publish a kind:10002 \
                 that would clear the author_relay_lists cache for this user"
                    .into(),
            ));
        }
        // Reject input that produces zero canonical tags (every URL was
        // malformed). Reaching the actor with a zero-tag event would emit
        // a valid kind:10002 that clears the cache — the same destructive
        // op the empty-input guard above blocks.
        let event = build_relay_list_event(&action.relays);
        if event.tags.is_empty() {
            return Err(ActionRejection::Invalid(
                "no canonical wss:// / ws:// relay URLs in input".into(),
            ));
        }
        Ok(())
    }

    fn execute(
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        let event = build_relay_list_event(&action.relays);
        // kind:10002 is a NIP-65 replaceable event — route through the
        // kernel's Auto path. For the *first* kind:10002 the author ever
        // publishes there is no NIP-65 outbox yet, so Auto falls back to
        // the bootstrap discovery relays (chicken-and-egg solved). For
        // updates, the existing outbox is used.
        //
        // Thread the registry-minted `correlation_id` so the publish
        // engine reports it in `action_results` and the host spinner that
        // fired on `dispatch_action` can be cleared with a terminal
        // verdict. Without this the dispatch arm never records a
        // terminal stage and the spinner hangs forever.
        send(ActorCommand::PublishUnsignedEvent {
            event,
            correlation_id: Some(correlation_id.to_string()),
        });
        Ok(())
    }
}

/// Register the `nmp.nip65.publish_relay_list` action module on the app.
pub fn register_actions(app: &mut nmp_core::NmpApp) {
    app.register_action::<PublishRelayListAction>();
}

#[cfg(test)]
#[path = "publish_relay_list_tests.rs"]
mod tests;

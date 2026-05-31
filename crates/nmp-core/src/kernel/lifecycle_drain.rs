//! Subscription-lifecycle drain helpers + relay-URL role lookup.
//!
//! Step 3 of `docs/architecture/crate-boundaries.md` (V-50) — survivors
//! of the deleted `kernel/outbox.rs` that are NOT routing decisions:
//!
//! - [`Kernel::drain_lifecycle_tick`] — actor idle-loop bridge (T142).
//!   Drains one tick of the [`crate::subs::SubscriptionLifecycle`] and
//!   returns the resulting wire frames. Builds a fresh
//!   [`super::mailboxes::KernelMailboxes`] adapter per call so the
//!   planner sees the live substrate cache + DM-relay map.
//! - [`Kernel::drain_lifecycle_outbound`] — wasm/`KernelReducer` bridge
//!   (V-04 Stage 2). Same as above but also stamps `RelayRole` lanes
//!   and converts to [`crate::relay::OutboundMessage`] so the
//!   non-actor (no-idle-loop) wasm path can hand frames straight to
//!   the transport.
//! - [`Kernel::role_for_relay_url`] — `RelayRole` lookup against the
//!   [`crate::kernel::AppRelay`] projection. Diagnostic lane label,
//!   not a routing input.

use nmp_planner::selection::relay_score_lookup::RelayAuthorScoreLookup;

use super::mailboxes::KernelMailboxes;
use super::Kernel;

impl Kernel {
    /// T142 — actor idle-loop bridge: drain one tick of the
    /// subscription lifecycle and return wire frames.
    ///
    /// Builds a [`KernelMailboxes`] adapter from the injected substrate
    /// [`crate::substrate::MailboxCache`] (post-step-3 source of truth
    /// for NIP-65 data) + the injected substrate
    /// [`crate::substrate::DmInboxRelayLookup`] (post-V-40 source of
    /// truth for DM-inbox relays; concrete cache lives in `nmp-nip17`).
    ///
    /// Per D8: an empty trigger inbox is a zero-cost no-op (no
    /// allocation, no compile pass). This is the common case on a quiet
    /// idle tick.
    pub(crate) fn drain_lifecycle_tick(&mut self) -> Vec<crate::subs::WireFrame> {
        let mailboxes = KernelMailboxes::new(
            std::sync::Arc::clone(&self.mailbox_cache),
            self.dm_inbox_relays_arc(),
        );
        // W4 §8.6: build a `ScoreLookupRef` via a split-borrow pattern.
        // We cannot write `let lookup = self as &dyn Trait` here because that
        // borrows all of `self` (including `lifecycle`) before the subsequent
        // `&mut self.lifecycle` in `drain_tick`. Instead we extract the two
        // read-only pieces we need — `&self.relay_score_map` and the current
        // wall-clock second — before the mutable call. `now_secs` is a
        // `u64` copy (not a borrow), so no lifetime escapes through it.
        //
        // A6 same-tick visibility: W3 writes `relay_score_map` synchronously
        // in the same actor tick before idle-tick drain fires, so the lookup
        // sees the latest scores.
        let now = self.now_secs();
        let score_lookup = Kernel::score_lookup_ref_from(&self.relay_score_map, now);
        let lookup: &dyn RelayAuthorScoreLookup = &score_lookup;
        self.lifecycle
            .drain_tick_with_lookup(&mailboxes, Some(lookup))
    }

    /// V-04 Stage 2 — `KernelReducer` / wasm bridge: drain one
    /// lifecycle tick and convert the resulting
    /// [`crate::subs::WireFrame`]s into
    /// [`crate::relay::OutboundMessage`]s ready to hand to the
    /// transport.
    ///
    /// This is the wasm/`KernelReducer`-side analogue of the native
    /// actor's `wire_frames_to_outbound` bridge
    /// (`actor/outbound.rs`). It exists because `KernelReducer` (used
    /// by `nmp-wasm`) does NOT have an actor idle loop; without an
    /// inline conversion, a `CompileTrigger::ViewOpened` enqueued by a
    /// `startup_requests`-style helper would never be drained on the
    /// wasm path and the REQs would never reach the wire.
    ///
    /// Empty inbox / empty diff is a zero-cost no-op (returns
    /// `Vec::new()` before allocating anything) — matches D8.
    ///
    /// Frame-to-outbound conversion is byte-for-byte the same as
    /// `actor::outbound::wire_frames_to_outbound`: same `["REQ",
    /// sub_id, filter]` / `["CLOSE", sub_id]` shape, same canonical URL
    /// stamp, same `RelayRole::Content` fallback for unrecognized relay
    /// URLs, same `register_planner_wire_frames` call so EOSE /
    /// keep-live bookkeeping matches the native path exactly. The
    /// duplication is deliberate — `wire_frames_to_outbound` is
    /// `pub(super)` to `actor` and crosses a module boundary the
    /// kernel must not depend on (D0). If this method ever drifts from
    /// the actor bridge, the `actor::outbound::tests` regression on
    /// canonicalization
    /// (`non_canonical_wire_frame_url_is_canonicalized_on_outbound`)
    /// is the canary — port any fix here too.
    pub(crate) fn drain_lifecycle_outbound(&mut self) -> Vec<crate::relay::OutboundMessage> {
        let frames = self.drain_lifecycle_tick();
        if frames.is_empty() {
            return Vec::new();
        }
        self.register_planner_wire_frames(&frames);
        frames
            .into_iter()
            .map(|f| {
                let (relay_url, text) = match f {
                    crate::subs::WireFrame::Req {
                        relay_url,
                        sub_id,
                        filter_json,
                        ..
                    } => (relay_url, format!(r#"["REQ","{sub_id}",{filter_json}]"#)),
                    crate::subs::WireFrame::Close { relay_url, sub_id } => {
                        (relay_url, format!(r#"["CLOSE","{sub_id}"]"#))
                    }
                };
                let relay_url = crate::relay::canonical_relay_url(&relay_url).unwrap_or(relay_url);
                let role = self
                    .role_for_relay_url(&relay_url)
                    .unwrap_or(crate::relay::RelayRole::Content);
                crate::relay::OutboundMessage {
                    role,
                    relay_url,
                    text,
                }
            })
            .collect()
    }

    /// T142 — role lookup: map a resolved relay URL to its `RelayRole` lane.
    ///
    /// Option A from the spec §3.2: bootstrap-URL matching with Content
    /// fallback. The two bootstrap seeds are the only URLs with known
    /// role assignments at this stage; any other URL (per-author NIP-65
    /// write relay resolved by the planner) falls through to
    /// `RelayRole::Content`, which accepts generic content-fetch REQs
    /// safely. This is correct because the planner only generates REQs
    /// for the content lane today (M2 scope).
    ///
    /// T105 / T-relay-url-normalize: the `url` argument is canonicalized
    /// before it is compared against `AppRelay.url`. `add_relay`
    /// always stores the canonical form (lowercase scheme+host,
    /// empty-path trailing slash stripped), so a raw, user-typed or
    /// non-canonical caller input — e.g. a kind:10002 NIP-65 write
    /// relay with a mixed-case host — would otherwise silently miss the
    /// matching edit row and fall through to the Content fallback,
    /// mislabelling an `indexer` relay's transport lane. The role is a
    /// diagnostic lane label only (T105), so a miss is not a routing
    /// fault, but the canonicalized compare keeps the projected lane
    /// accurate.
    ///
    /// M11 will sharpen this to a per-URL lookup once the URL→role index
    /// is maintained by the relay-lifecycle manager.
    // M11 will add a `None` path when the URL is unknown; the Option is
    // intentionally forward-reserved so call sites don't need signature churn.
    #[allow(clippy::unnecessary_wraps)]
    pub(crate) fn role_for_relay_url(&self, url: &str) -> Option<crate::relay::RelayRole> {
        use crate::relay::RelayRole;
        // Canonicalize so a raw/non-canonical input matches the canonical
        // `AppRelay.url` keys. Fall back to the raw string for inputs that
        // do not parse as ws/wss (no edit row will match those anyway).
        let lookup = crate::relay::canonical_relay_url(url).unwrap_or_else(|| url.to_string());
        for row in &self.configured_relays {
            if row.url == lookup {
                if crate::actor::has_role(&row.role, "indexer") {
                    return Some(RelayRole::Indexer);
                }
                if crate::actor::has_role(&row.role, "read")
                    || crate::actor::has_role(&row.role, "write")
                {
                    return Some(RelayRole::Content);
                }
            }
        }
        // Returns `Some` unconditionally today (Content fallback). The `Option`
        // return is retained so M11's per-URL index can distinguish "no role
        // known for this URL" (`None`) from "explicitly Content" without a
        // signature change at every call site.
        Some(RelayRole::Content)
    }
}

//! [`AttributionPayload`] — the per-instance metadata an engine consumer
//! attaches to a thread root when a *qualifying* event references it.
//!
//! The engine ([`crate::RootIndexedFeed`]) is generic over this trait so the
//! `nmp-feed` crate never names a protocol convention: it does not know what
//! "a reply" is, what "a follow" is, or how a profile decodes. The protocol
//! instance crate supplies the concrete type. **No protocol-named token may
//! appear in this file or anywhere under `crates/nmp-feed/src/` — a CI grep
//! gate enforces D0** (see `crates/nmp-testing/tests/op_feed_doctrine_lint.rs`).

use nmp_core::substrate::KernelEvent;

/// Per-root attribution metadata produced from a qualifying referencing event.
///
/// An implementation decides — entirely inside [`Self::from_reply`] — whether
/// a given event qualifies (correct kind, references a parent, authored by a
/// followed pubkey, …). Returning `None` drops the event from attribution.
///
/// The associated [`Self::Profile`] type is the **B1 dependency-cycle fix**:
/// the engine refreshes display data in place via
/// [`Self::refresh_for_profile`] without `nmp-feed` ever naming the instance's
/// concrete profile type. The engine only knows "there is some profile type
/// for this payload".
pub trait AttributionPayload: Clone + Send + Sync + 'static {
    /// Opaque profile type owned by the instance crate. The engine caches and
    /// fans these out to [`Self::refresh_for_profile`] but never inspects them.
    type Profile: Clone + Send + Sync + 'static;

    /// Build attribution from a referencing event, or `None` if the event does
    /// not qualify.
    ///
    /// * `follow` — predicate over a pubkey; the engine passes its
    ///   construction-time follow closure. The implementation decides whether
    ///   to consult it (the engine ALSO gates on follow before calling, so a
    ///   trivially-`true` re-check is acceptable).
    /// * `profile_for` — best-effort profile lookup for a pubkey; `None` when
    ///   no profile is cached yet (display fills in later via
    ///   [`Self::refresh_for_profile`]).
    fn from_reply(
        reply: &KernelEvent,
        follow: &dyn Fn(&str) -> bool,
        profile_for: &dyn Fn(&str) -> Option<Self::Profile>,
    ) -> Option<Self>;

    /// Event id of the referencing event this attribution was built from. Used
    /// as the per-root sub-map key so a re-delivered reply de-dupes.
    fn reply_event_id(&self) -> &str;

    /// Pubkey of the referencing event's author. Used for profile fan-out.
    fn author_pubkey(&self) -> &str;

    /// Signed `created_at` of the referencing event. Used for per-root
    /// eviction ordering (oldest reply evicted first under D5 pressure).
    fn reply_created_at(&self) -> u64;

    /// Refresh display fields in place when a newer profile for this
    /// attribution's author arrives. Must not change the keying fields
    /// ([`Self::reply_event_id`], [`Self::author_pubkey`]).
    fn refresh_for_profile(&mut self, profile: &Self::Profile);
}

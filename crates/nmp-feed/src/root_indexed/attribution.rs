//! [`AttributionPayload`] — the per-instance metadata an engine consumer
//! attaches to a thread root when a *qualifying* event references it.
//!
//! The engine ([`crate::RootIndexedFeed`]) is generic over this trait so the
//! `nmp-feed` crate never names a protocol convention: it does not know what
//! "a reply" is, what "a follow" is, or how secondary data is fetched. The
//! protocol instance crate supplies the concrete type. **No protocol-named
//! token may appear in this file or anywhere under `crates/nmp-feed/src/` — a
//! CI grep gate enforces D0** (see
//! `crates/nmp-testing/tests/op_feed_doctrine_lint.rs`).

use nmp_core::substrate::KernelEvent;

/// Per-root attribution metadata produced from a qualifying referencing event.
///
/// An implementation decides — entirely inside [`Self::from_reply`] — whether
/// a given event qualifies (correct kind, references a parent, authored by a
/// followed pubkey, …). Returning `None` drops the event from attribution.
///
pub trait AttributionPayload: Clone + Send + Sync + 'static {
    /// Build attribution from a referencing event, or `None` if the event does
    /// not qualify.
    ///
    /// * `follow` — predicate over a pubkey; the engine passes its
    ///   construction-time follow closure. The implementation decides whether
    ///   to consult it (the engine ALSO gates on follow before calling, so a
    ///   trivially-`true` re-check is acceptable).
    fn from_reply(reply: &KernelEvent, follow: &dyn Fn(&str) -> bool) -> Option<Self>;

    /// Event id of the referencing event this attribution was built from. Used
    /// as the per-root sub-map key so a re-delivered reply de-dupes.
    fn reply_event_id(&self) -> &str;
}

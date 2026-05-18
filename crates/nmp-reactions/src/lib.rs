//! `nmp-reactions` — NIP-25 reactions (kind:7) + NIP-18 reposts (kind:6 /
//! generic kind:16) as an NMP protocol crate.
//!
//! Implements the design recommendation in `docs/design/kind-wrappers.md` §3:
//! the read side is a pure `try_from_event` decoder producing an immutable
//! [`SocialRecord`]; the write side is a set of builders that produce an
//! `UnsignedEvent`. Read + write share no mutable state — there is no
//! NDK-style setter (the D4 violation `kind-wrappers.md` §9 #2 forbids).
//!
//! ## Regular events, not replaceable
//!
//! Kinds 7 / 6 / 16 are regular events. NIP-33-style `(author, d_tag)`
//! supersession does NOT apply. The nip23 "stale redelivery" guard maps here to
//! plain **duplicate-`event_id` idempotency** — the same event id ingested
//! twice never double-counts (see [`domain`] and [`view::ReactionAccumulator`]).
//!
//! ## Module layout
//!
//! - [`kinds`] — `KIND_REACTION = 7`, `KIND_REPOST = 6`,
//!   `KIND_GENERIC_REPOST = 16`.
//! - [`decode`] — [`SocialRecord`] (single struct, [`SocialKind`]-tagged) +
//!   `try_from_event` / `try_from_kernel_event` through a shared decode core.
//! - [`build`] — `Reaction::to_event(...)` / `Reaction::to_address(...)` /
//!   `Repost::of(...)` / `GenericRepost::of(...)` → `UnsignedEvent`, with a
//!   typed [`ReactionBuildError`] (D6).
//! - [`domain`] — [`ReactionsDomain`]: composite reverse indexes (`by_target`,
//!   `by_target_content`, `by_reactor`), `event_id` idempotency, and
//!   [`reaction_summary`] with per-`(reactor, target)` newest-wins collapse.
//! - [`view`] — [`view::ReactionSummaryView`] + [`view::RepostsView`].
//!
//! ## Phase-1 ingest dispatch gap
//!
//! Per `kind-wrappers.md` §6 + §8 + PD-008, decoded records are cached in the
//! domain store at ingest time — `ReactionsDomain` declares
//! `ingest_kinds() = &[7, 6, 16]` and the kernel dispatch table calls
//! `decode_and_route` per insert. The kernel-side dispatch table is a separate
//! Phase 1 deliverable; `decode_and_route` is callable directly today and is
//! exercised by the integration tests.

pub mod build;
pub mod decode;
pub mod domain;
pub mod kinds;
pub mod view;

pub use build::{
    GenericRepost, GenericRepostBuilder, Reaction, ReactionBuildError, ReactionBuilder, Repost,
    RepostBuilder,
};
pub use decode::{
    try_from_event, try_from_kernel_event, EmojiRef, ReactionTarget, SocialKind, SocialRecord,
};
pub use domain::{
    decode_and_route, get, list_by_reactor, list_for_target, reaction_summary, ReactionSummary,
    ReactionsDomain, NAMESPACE,
};
pub use kinds::{KIND_GENERIC_REPOST, KIND_REACTION, KIND_REPOST, SOCIAL_KINDS};
pub use view::{
    ReactionAccumulator, ReactionSummaryPayload, ReactionSummarySpec, ReactionSummaryView,
    ReactionViewDelta, RepostsPayload, RepostsSpec, RepostsView,
};

use nmp_core::substrate::ModuleRegistry;

/// Register every module produced by `nmp-reactions` into a kernel
/// `ModuleRegistry`. Called by per-app generated code (`nmp-codegen`).
pub fn register(registry: &mut ModuleRegistry) {
    registry.register_domain::<ReactionsDomain>();
    view::register_all(registry);
}

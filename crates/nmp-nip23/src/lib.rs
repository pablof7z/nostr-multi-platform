//! `nmp-nip23` — NIP-23 long-form articles (kind:30023) as an NMP protocol crate.
//!
//! Implements the design recommendation in `docs/design/kind-wrappers.md` §3
//! (worked example): the read side is a pure `try_from_event` decoder; the
//! write side is an `ArticleBuilder` that produces an `UnsignedEvent`. Read +
//! write share no mutable state — there is no NDK-style `article.title = "x"`
//! setter, which would violate D4 (single writer per fact).
//!
//! ## Module layout
//!
//! - [`kinds`] — `KIND_LONG_FORM_ARTICLE = 30023`.
//! - [`decode`] — `ArticleRecord` + `try_from_event(&StoredEvent)` (pure fn,
//!   no I/O, no allocations beyond the record itself).
//! - [`build`] — `Article::new(d).title(…)…build(author, ts)` →
//!   `UnsignedEvent`. Validates required fields per D6 with typed
//!   `ArticleBuildError`.
//! - [`domain`] — composite-key reverse indexes for kind:30023 under the
//!   `nmp.nip23.articles` namespace (`by_author` /
//!   `by_d_tag(author, d_tag)`), used by the view layer. Exposes
//!   `decode_and_route` for the kernel ingest dispatch
//!   (Phase 1 — see §6 in the design doc).
//! - [`view`] — `ArticleListView` + `ArticleDetailView`.
//!
//! ## Ingest dispatch
//!
//! Per `docs/design/kind-wrappers.md` §6 + §8 + PD-008, decoded records are
//! cached in the domain store **at ingest time**. Callers (apps or
//! `KernelEventObserver` impls) dispatch kind:30023 events to
//! `decode_and_route`, which writes the decoded `ArticleRecord` under the
//! composite reverse indexes (`by_author` / `by_d_tag(author, d_tag)`).

// NIP-23 surface is feature-gated behind `long-form`. The crate has zero
// app callers (no ActionModule, no KernelEventObserver, no subscription
// filter) and is referenced only from test/fixture crates. Gating prevents
// the inert surface from misleading future contributors while preserving
// the kind:30023 integration-test path via `--features long-form`.
#[cfg(feature = "long-form")]
pub mod build;
#[cfg(feature = "long-form")]
pub mod decode;
#[cfg(feature = "long-form")]
pub mod domain;
#[cfg(feature = "long-form")]
pub mod kinds;
#[cfg(feature = "long-form")]
pub mod view;

#[cfg(feature = "long-form")]
pub use build::{Article, ArticleBuildError, ArticleBuilder};
#[cfg(feature = "long-form")]
pub use decode::{try_from_event, ArticleRecord};
#[cfg(feature = "long-form")]
pub use domain::{decode_and_route, get, list_all, list_by_author, NAMESPACE};
#[cfg(feature = "long-form")]
pub use kinds::KIND_LONG_FORM_ARTICLE;
#[cfg(feature = "long-form")]
pub use view::{
    ArticleAccumulator, ArticleDetailPayload, ArticleDetailSpec, ArticleDetailView,
    ArticleListPayload, ArticleListSpec, ArticleListView, ArticleViewDelta, PublicKey,
};

// NOTE: `nmp-nip23` exposes its view types (`ArticleListView`,
// `ArticleDetailView`) as plain public types whose `open` / `on_event_*` /
// `snapshot` inherent methods are reached via static dispatch — the
// `ViewModule` trait and the former `register(&mut ModuleRegistry)` entry
// point were both deleted because no kernel-side registry ever drove them.
// The live extension path is `KernelEventObserver` — see
// `nmp_core::substrate` module docs.

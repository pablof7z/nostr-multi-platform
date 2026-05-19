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
//! - [`domain`] — `ArticlesDomain: DomainModule` for the
//!   `nmp.nip23.articles` namespace. Owns the `by_author` /
//!   `by_d_tag(author, d_tag)` composite-key reverse indexes used by the
//!   view layer. Exposes `decode_and_route` for the kernel ingest dispatch
//!   (Phase 1 — see §6 in the design doc).
//! - [`view`] — `ArticleListView` + `ArticleDetailView`.
//!
//! ## Phase-1 ingest dispatch gap
//!
//! Per `docs/design/kind-wrappers.md` §6 + §8 + PD-008, decoded records are
//! cached in the domain store **at ingest time** — `ArticlesDomain` declares
//! `ingest_kinds() = &[30023]` and the kernel dispatch table calls
//! `decode_and_route` per insert. The kernel-side dispatch table itself is a
//! separate Phase 1 deliverable; `decode_and_route` is callable directly today
//! and is exercised by the integration tests, so apps can wire ingest manually
//! until the kernel routing lands.

pub mod build;
pub mod decode;
pub mod domain;
pub mod kinds;
pub mod view;

pub use build::{Article, ArticleBuildError, ArticleBuilder};
pub use decode::{try_from_event, ArticleRecord};
pub use domain::{decode_and_route, get, list_all, list_by_author, ArticlesDomain, NAMESPACE};
pub use kinds::KIND_LONG_FORM_ARTICLE;
pub use view::{
    ArticleAccumulator, ArticleDetailPayload, ArticleDetailSpec, ArticleDetailView,
    ArticleListPayload, ArticleListSpec, ArticleListView, ArticleViewDelta, PublicKey,
};

use nmp_core::substrate::ModuleRegistry;

/// Register every module produced by `nmp-nip23` into a kernel
/// `ModuleRegistry`. Called by per-app generated code (`nmp-codegen`) so the
/// kernel knows the crate's domain + view populations exist.
pub fn register(registry: &mut ModuleRegistry) {
    registry.register_domain::<ArticlesDomain>();
    view::register_all(registry);
}

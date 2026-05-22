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
//!
//! Prior `domain` (composite-key reverse indexes) and `view`
//! (`ArticleListView` / `ArticleDetailView`) modules were deleted: both had
//! zero external callers — no `ActionModule`, no `KernelEventObserver`, no
//! subscription filter, no app dispatch. The live extension path is
//! `KernelEventObserver` — see `nmp_core::substrate` module docs.

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
pub mod kinds;

#[cfg(feature = "long-form")]
pub use build::{Article, ArticleBuildError, ArticleBuilder};
#[cfg(feature = "long-form")]
pub use decode::{try_from_event, ArticleRecord};
#[cfg(feature = "long-form")]
pub use kinds::KIND_LONG_FORM_ARTICLE;

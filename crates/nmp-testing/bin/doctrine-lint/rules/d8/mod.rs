//! D8 — no-polling reactivity-contract lint.
//!
//! D8 originally bundled two checks: a hot-path-allocation lint (path-scoped
//! to `crates/nmp-core/src/kernel/ingest/`, opt-in via a `// hot path`
//! marker comment) and this crate's [`no_polling`] check. The hot-path-
//! allocation half was deleted (see #2761 / #2769): the `// hot path`
//! marker was used by zero functions in its scoped directory, so the check
//! measured nothing — it was permanently-vacuous machinery, not a gap that
//! annotation could close. The real ingest hot path (`kernel::ingest`)
//! allocates heavily in cold/error branches; annotating the genuinely hot
//! entry point would have required a large per-event-allocation elimination
//! refactor that is out of scope for a doctrine-lint enforcement-surface
//! fix. The doc comment for the deleted check already earmarked the real
//! solution: a future dhat-rs-backed per-event allocation-count gate
//! (tracked as a new issue, not a grep heuristic).
//!
//! [`no_polling`] — no `sleep`+check loops anywhere in production code — is
//! the sole surviving D8 check: global and unconditional, unlike the
//! deleted function-scope-gated, path-scoped hot-path lint.

mod no_polling;

pub const ID: &str = "D8";

pub use no_polling::check_no_polling;

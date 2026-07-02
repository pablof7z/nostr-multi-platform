//! D8 — reactivity-contract lints, bundled under one rule id.
//!
//! D8 covers two independently-scoped checks that both trace back to the
//! same reactivity contract (ADRs 0001–0004 + `AGENTS.md` §reactivity-contract):
//!
//! - [`hot_path_allocation`] — no per-event allocation on the marked
//!   ingest hot path.
//! - [`no_polling`] — no `sleep`+check loops anywhere in production code.
//!
//! They are unrelated in *mechanism* (one is function-scope-gated and
//! path-scoped, the other is global and unconditional) but share the rule
//! id because both enforce "the kernel reacts, it does not poll or
//! allocate proportional to event volume". Each submodule documents its
//! own scope, banned tokens, and escape hatch; this file only composes
//! them behind the stable `d8::` entry points the driver calls.

mod hot_path_allocation;
mod no_polling;

pub const ID: &str = "D8";

pub use hot_path_allocation::{check_in_scope, file_in_scope, HotPathTracker};
pub use no_polling::check_no_polling;

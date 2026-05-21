//! Substrate — the per-protocol extension contracts (`ViewModule`,
//! `ActionModule`, `DomainModule`, `CapabilityModule`, `IdentityModule`).
//!
//! # Extension mechanism: v1 vs v2
//!
//! The five traits in this module are the **v2** extension design — a
//! family of typed, namespace-keyed modules the kernel would discover and
//! drive through a dispatch runtime. That runtime does not exist yet. The
//! per-NIP crates implement these traits and tests invoke their methods
//! directly (static dispatch — `<RepliesView as ViewModule>::open(...)`),
//! so the trait *contracts* are real and load-bearing. What never shipped
//! is a kernel-side registry that stores `dyn Trait` objects and fans
//! events to them.
//!
//! A previous iteration shipped a `ModuleRegistry` that *looked* like that
//! runtime but only collected `(namespace, family, type_name)` strings —
//! nothing in the kernel, the actor, or codegen ever read them back. It
//! has been removed; it was documentation theater that misled readers
//! about how extension actually works today.
//!
//! ## v1 extension mechanism: `KernelEventObserver`
//!
//! The mechanism the kernel *actually* drives in v1 is
//! [`KernelEventObserver`](crate::KernelEventObserver) — a flat raw-event
//! fan-out. Per-app crates register `Arc<dyn KernelEventObserver>`
//! observers; the kernel fans every accepted event (`Inserted | Replaced`)
//! to all registered observers. This is what Chirp's modular timeline and
//! the Marmot projection use today.
//!
//! Canonical pattern:
//! - the slot + registration helpers: `actor/commands/event_observer.rs`
//! - the kernel fan-out integration: `kernel/event_observer.rs`
//! - a per-app crate registering an observer: `nmp-app-chirp/src/ffi.rs`

mod action;
mod capability;
mod domain;
mod identity;
mod keyring;
pub mod placeholder;
mod view;

pub use action::{
    ActionContext, ActionId, ActionInput, ActionModule, ActionPlan, ActionRejection, ActionResult,
    ActionStatus, ActionTransition,
};
pub use capability::{CapabilityEnvelope, CapabilityModule, CapabilityRequest};
pub use domain::{DomainIndex, DomainMigration, DomainModule, MigrationTx};
pub use identity::{
    BoxFuture, IdentityContext, IdentityError, IdentityId, IdentityModule, IdentityScopeKind,
    SignedEvent, SigningError, UnsignedEvent,
};
pub use keyring::{
    KeyringCapability, KeyringIdentityWiring, KeyringRequest, KeyringResult, KeyringStatus,
    MALFORMED_RESULT,
};
pub use placeholder::{picture_placeholder, Placeholder};
pub use view::{EventId, KernelEvent, ProjectionChange, ViewContext, ViewDependencies, ViewModule};

// NIP-10 / tag codec lives in `crate::tags` (a protocol codec, like nip19 /
// nip21 — not a per-kind decoder, so D0-clean). Re-exported here so the
// per-NIP relation crates that already `use nmp_core::substrate::{...}`
// consume one source.
pub use crate::tags::{
    a_tag, all_tag_values, e_tag, first_tag_value, p_tag, parse_nip10, q_tag, EventRef, Nip10Refs,
};

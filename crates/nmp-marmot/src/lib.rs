//! `nmp-marmot` — Marmot Protocol (MLS-over-Nostr) as an NMP protocol crate.
//!
//! Adapter only — does NOT re-implement MLS. Wraps `mdk-core` 0.8.0 +
//! `mdk-sqlite-storage` 0.8.0. Spec: `docs/plan/marmot-mls.md`,
//! API surface: `docs/research/mdk-api.md`.
//!
//! ## Kernel-boundary exit gate
//!
//! - `nmp-core` gains zero MLS types. All MLS / MDK types stay inside this
//!   crate. `nmp-marmot` is the SOLE importer of `mdk-core` / `openmls`.
//! - No other NMP crate depends on MLS types. The substrate module impls
//!   (`domain` / `view` / `action`) expose only NMP-native record / payload /
//!   plan shapes; MDK types appear only in [`service`], which is consumed
//!   in-crate (tests) and by a future actor/FFI bridge.
//!
//! ## Two-layer architecture
//!
//! 1. **Substrate module layer** ([`domain`], [`view`], [`action`]) — mirrors
//!    `nmp-nip29`. Stateless trait impls registered into the kernel
//!    `ModuleRegistry`. ActionModules emit an unsigned [`action::PublishPlan`]
//!    + [`action::RelayPin`] carrier; the actor's signer-bridge signs. These
//!    impls carry NO MDK types — they satisfy registry wiring + the
//!    kernel-boundary grep.
//! 2. **Service layer** ([`service::MarmotService`]) — the real MDK-driving
//!    API. Holds an `MDK<S>` + `nostr::Keys`. This is what the in-crate
//!    round-trip tests exercise and what a headless integration-test driver
//!    (and the future actor) hold. MDK is synchronous; callers in an async
//!    context offload via the runtime's blocking bridge (not this crate's
//!    concern — the service is sync `&self`).
//!
//! ## Welcome (kind:444) delivery + NIP-59
//!
//! The service drives NIP-59 gift-wrap / unwrap of kind:444 Welcome rumors
//! through `nmp_nip59::{gift_wrap, unwrap_gift_wrap}` (the M11.5 key-boundary
//! seam — caller holds `nostr::Keys`). The actor's signer-bridge will later
//! recognise the `WrapPlan` carrier and perform the wrap where the
//! `KeyringCapability` is available.
//!
//! ## Relay routing
//!
//! Group events (kind:445) are relay-pinned to the group relay via the
//! [`action::PublishPlan`] / [`action::RelayPin`] carrier (ADR-0012,
//! `InterestShape::relay_pin`). KeyPackage events (kind:30443/443) use
//! standard author-write outbox routing. Interest helpers live in
//! [`interest`].

pub mod action;
pub mod domain;
pub mod interest;
pub mod projection;
pub mod service;
pub mod view;

/// Re-exports of the handful of `mdk-core` types that appear in the public
/// [`service::MarmotService`] signature. Callers that drive the service
/// (round-trip tests in-crate; the diagnostic REPL out-of-crate) need to
/// construct these without taking a direct `mdk-core` dependency — the
/// kernel-boundary exit gate ("`nmp-marmot` is the SOLE importer of
/// `mdk-core`/`openmls`") would otherwise force every caller across the
/// boundary to import `mdk-core` themselves.
///
/// Add types here ONLY when they appear in `service`'s public API. This
/// module deliberately does NOT re-export the wider MDK surface.
pub mod mls_types {
    pub use mdk_core::prelude::{GroupId, MessageProcessingResult, NostrGroupConfigData};
}

/// Register every `nmp-marmot` substrate module into the kernel registry.
/// Called by per-app generated code (`nmp-codegen`) at startup.
pub fn register(registry: &mut nmp_core::substrate::ModuleRegistry) {
    domain::register_all(registry);
    view::register_all(registry);
    action::register_all(registry);
}

#[cfg(test)]
mod tests;

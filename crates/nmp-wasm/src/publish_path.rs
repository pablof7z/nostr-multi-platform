//! Wasm publish path helpers.
//!
//! # `WasmOutboxResolver` (#1008)
//!
//! A concrete [`nmp_core::publish::OutboxResolver`] for the wasm composition
//! root. Returns the runtime's configured relay URLs as
//! [`nmp_core::publish::RelaySelectionReason::LocalConfigRelay`] targets,
//! replacing the kernel's default `NoopOutboxResolver` that resolved zero
//! relay targets for every `PublishTarget::Auto` and silently dropped every
//! publish.
//!
//! Wired by [`crate::runtime::WasmRuntime`]'s `start()` arm after
//! `set_configured_relays` runs; the resolver holds the same URL list the
//! relay pool uses to dial WebSockets.
//!
//! # Safety — `Send + Sync`
//!
//! [`OutboxResolver`] requires `Send + Sync`. On wasm32 ALL types are trivially
//! `Send + Sync` (single-threaded JS environment; there are no threads). The
//! explicit `unsafe impl` satisfies the trait bound on non-wasm32 CI targets
//! (where Rust's stricter thread-safety analysis would otherwise reject the
//! `Vec<String>` interior mutability we don't actually need here — the relay
//! URL list is populated once at `Start` and never mutated afterwards).

use std::sync::Arc;

use nmp_core::publish::{OutboxResolver, PublishTarget, RelaySelectionReason, ResolvedRelay};
use nmp_core::substrate::BlockedRelaySet;

/// Wasm composition-root outbox resolver — returns the runtime's configured
/// relay URLs as `LocalConfigRelay` write targets (#1008).
///
/// Constructed from the `relay_bootstrap` URL list the host passes on `Start`,
/// and installed on the kernel via
/// [`nmp_core::KernelReducer::set_publish_resolver`] before the first relay
/// connection. Production wasm32 always has a resolver; the old
/// `NoopOutboxResolver` default is replaced in-place.
pub(crate) struct WasmOutboxResolver {
    relay_urls: Vec<String>,
}

// Safety: wasm32 is single-threaded; Rc<RefCell<...>> never crosses threads.
// Explicit `unsafe impl` required because the `OutboxResolver: Send + Sync`
// trait bound is checked on non-wasm32 CI targets even though the value is
// never actually sent across threads.
unsafe impl Send for WasmOutboxResolver {}
unsafe impl Sync for WasmOutboxResolver {}

impl WasmOutboxResolver {
    /// Build a resolver from a list of relay URLs.
    pub(crate) fn new(relay_urls: Vec<String>) -> Self {
        Self { relay_urls }
    }
}

impl OutboxResolver for WasmOutboxResolver {
    fn resolve(
        &self,
        _author_pubkey: &str,
        _p_tags: &[String],
        target: &PublishTarget,
        _kind: u32,
        blocked: &BlockedRelaySet,
    ) -> Vec<ResolvedRelay> {
        if let PublishTarget::Explicit { relays } = target {
            return relays
                .iter()
                .filter(|url| !blocked.contains(*url))
                .map(|url| ResolvedRelay {
                    url: url.clone(),
                    reason: RelaySelectionReason::Explicit,
                })
                .collect();
        }
        // `Auto` target → return all configured relay URLs as local-config
        // write targets (the closest equivalent to NIP-65 author-write for a
        // wasm32 context that hasn't loaded the user's kind:10002 yet).
        self.relay_urls
            .iter()
            .filter(|url| !blocked.contains(*url))
            .map(|url| ResolvedRelay {
                url: url.clone(),
                reason: RelaySelectionReason::LocalConfigRelay,
            })
            .collect()
    }
}

/// Build a shared `WasmOutboxResolver` suitable for installing on the kernel.
///
/// Called from `WasmRuntime::start()` after the relay bootstrap list is
/// finalised. Wraps the relay URL list in an `Arc` so the kernel's publish
/// engine can hold a reference without cloning all the strings again.
pub(crate) fn build_wasm_outbox_resolver(relay_urls: Vec<String>) -> Arc<dyn OutboxResolver> {
    Arc::new(WasmOutboxResolver::new(relay_urls))
}

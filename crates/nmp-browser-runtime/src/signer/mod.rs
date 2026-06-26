//! Capability/signer-provider registry and sign-completion infrastructure for
//! the browser runtime (#2049 / #2065 / #2066 / #2067 / #2068).
//!
//! # Sub-modules
//!
//! - [`registry`] — `CapabilityProviderRegistry` + `CapabilityEnvelope`
//!   (#2049 / #2065).
//! - [`completion`] — `SignerCompletion` channel types + `broker_sign_request`
//!   helper (#2049 / #2066 / #2067).
//! - [`nip46`] — `BunkerBroker` wiring for native builds (#2068).
//!   On wasm32, NIP-46 is host-brokered (nmp-signer-broker is native-only).

pub(crate) mod completion;
pub(crate) mod registry;
/// NIP-46 bunker-broker wiring (native builds only — see module doc for
/// the wasm32 host-brokered path).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod nip46;

pub(crate) use completion::{
    broker_sign_request, enqueue_completion, SignerCompletion, SignerCompletionRx,
    SignerCompletionTx,
};
// `CapabilityEnvelope` is re-exported publicly so `lib.rs` can expose it as
// a crate-root type. `CapabilityProviderRegistry` stays `pub(crate)`.
pub use registry::CapabilityEnvelope;
pub(crate) use registry::CapabilityProviderRegistry;

//! Capability/signer-provider registry and sign-completion infrastructure for
//! the browser runtime (#2049 / #2065 / #2066 / #2067).
//!
//! # Sub-modules
//!
//! - [`registry`] — `CapabilityProviderRegistry` + `CapabilityEnvelope`
//!   (#2049 / #2065).
//! - [`completion`] — `SignerCompletion` channel types + `broker_sign_request`
//!   helper (#2049 / #2066 / #2067).
//!
//! NIP-46 (bunker://) provider wiring is #2068 (follow-up PR, out of scope here).

pub(crate) mod completion;
pub(crate) mod registry;

pub(crate) use completion::{
    broker_sign_request, enqueue_completion, SignerCompletion, SignerCompletionRx,
    SignerCompletionTx,
};
// `CapabilityEnvelope` is re-exported publicly so `lib.rs` can expose it as
// a crate-root type. `CapabilityProviderRegistry` stays `pub(crate)`.
pub use registry::CapabilityEnvelope;
pub(crate) use registry::CapabilityProviderRegistry;

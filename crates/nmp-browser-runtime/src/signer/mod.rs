//! Capability/signer-provider registry and sign-completion infrastructure for
//! the browser runtime (#2049 / #2065 / #2066 / #2067).
//!
//! # Sub-modules
//!
//! - [`registry`] — `CapabilityProviderRegistry` + `CapabilityEnvelope`
//!   (#2049 / #2065).
//! - [`completion`] — `SignerCompletion` channel types + `broker_sign_request`
//!   helper (#2049 / #2066 / #2067).
//! - [`cipher`] — pending NIP-44 provider completion support (#2195).
//! - [`nip46`] — browser NIP-46 bunker lifecycle bridge.

pub(crate) mod cipher;
pub(crate) mod completion;
pub(crate) mod nip46;
pub(crate) mod registry;

pub(crate) use cipher::{dispatch_nip44_cipher, Nip44CipherMode, PendingCipherCompletions};
pub(crate) use completion::{
    broker_sign_request, enqueue_completion, PendingSignerCompletions, SignerCompletion,
    SignerCompletionRx, SignerCompletionTx,
};
pub(crate) use nip46::BrowserNip46Runtime;
// `CapabilityEnvelope` is re-exported publicly so `lib.rs` can expose it as
// a crate-root type. `CapabilityProviderRegistry` stays `pub(crate)`.
pub use registry::CapabilityEnvelope;
pub(crate) use registry::CapabilityProviderRegistry;

//! # nmp-signer-iface
//!
//! Transport interface types shared by [`nmp-core`] and [`nmp-signers`].
//!
//! This crate is intentionally a **leaf** — it has no workspace dependencies.
//! It exists to break what would otherwise be a circular doctrine **D0**
//! violation: `nmp-core` needs to hold a trait object whose methods refer to
//! [`SignerOp`], but `nmp-core` must not depend on `nmp-signers`.  Hoisting
//! the small set of shared interface types here lets both sides import what
//! they need without violating D0. It also owns the dependency-light NIP-01
//! event value types ([`SignedEvent`] / [`UnsignedEvent`] / [`SigningError`])
//! and the [`RemoteSignerHandle`] trait, so lower-layer signer/protocol crates
//! can name the signing substrate vocabulary without depending on the kernel
//! (issue #1720). `nmp-core` re-exports them so its own and protocol-crate
//! import paths are unchanged.
//!
//! ## Contents
//!
//! - [`SignerError`] — the canonical error type returned by every fallible
//!   signer operation.  String-typed by design (doctrine **D6** — errors never
//!   cross FFI as exceptions; this is for Rust-internal flow only).
//! - [`SignerOp`] — pollable thunk for ops that may complete asynchronously.
//!   Lets the kernel actor poll signer ops on its existing `std::sync::mpsc`
//!   loop without pulling in Tokio.
//! - [`Nip46Rpc`] + [`Nip46Transport`] — the outbound contract a NIP-46 signer
//!   uses to ask the kernel to send a kind:24133 event on its behalf.
//! - [`SignedEvent`] / [`UnsignedEvent`] / [`SigningError`] — the dependency-light
//!   NIP-01 event value types every signer and the publish pipeline exchange.
//! - [`RemoteSignerHandle`] — the actor-facing trait for out-of-kernel signers.
//!
//! [`nmp-core`]: https://docs.rs/nmp-core
//! [`nmp-signers`]: https://docs.rs/nmp-signers

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod error;
pub mod external_signer_transport;
pub mod handle;
pub mod nip46_transport;
pub mod op;
pub mod signing;

pub use error::SignerError;
pub use external_signer_transport::{
    ExternalSignerMethod, ExternalSignerOutcome, ExternalSignerRequest, ExternalSignerResponse,
    ExternalSignerTransport, Nip55Permission, EXTERNAL_SIGNER_NAMESPACE, EXTERNAL_SIGN_TIMEOUT,
    PENDING_SIGN_TIMEOUT,
};
pub use handle::RemoteSignerHandle;
pub use nip46_transport::{Nip46Rpc, Nip46Transport};
pub use op::SignerOp;
pub use signing::{SignedEvent, SigningError, UnsignedEvent};

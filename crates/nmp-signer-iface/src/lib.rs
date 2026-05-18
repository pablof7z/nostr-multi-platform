//! # nmp-signer-iface
//!
//! Transport interface types shared by [`nmp-core`] and [`nmp-signers`].
//!
//! This crate is intentionally a **leaf** — it has no workspace dependencies.
//! It exists to break what would otherwise be a circular doctrine **D0**
//! violation: `nmp-core` needs to hold a trait object whose methods refer to
//! [`SignerOp`], but `nmp-core` must not depend on `nmp-signers`.  Hoisting
//! the small set of shared interface types here lets both sides import what
//! they need without violating D0.
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
//!
//! [`nmp-core`]: https://docs.rs/nmp-core
//! [`nmp-signers`]: https://docs.rs/nmp-signers

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod error;
pub mod nip46_transport;
pub mod op;

pub use error::SignerError;
pub use nip46_transport::{Nip46Rpc, Nip46Transport};
pub use op::SignerOp;

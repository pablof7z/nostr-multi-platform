//! Signer trait + concrete implementations.
//!
//! The `Signer` trait is intentionally minimal (applesauce shape).  Encryption
//! schemes (NIP-04, NIP-44) are optional namespaces because real-world signers
//! genuinely have different capability sets — extension signers may expose only
//! one scheme; readonly signers expose none.
//!
//! Async surface is delivered via `SignerOp<T>` — our own pollable thunk type
//! that avoids forcing a Tokio executor into the kernel actor loop.

mod local;
mod nip46;
mod nip07;
mod op;
mod payload;
mod traits;

pub use local::LocalKeySigner;
pub use nip46::{Nip46Rpc, Nip46Signer, Nip46SignerHandle, Nip46Transport};
pub use nip07::Nip07Signer;
pub use op::SignerOp;
pub use payload::{LocalKeyMaterial, LocalPayload, Nip46Payload, Nip07Payload, SignerPayload};
pub use traits::{Nip04, Nip44, Signer, SignerBackend, SignerError};

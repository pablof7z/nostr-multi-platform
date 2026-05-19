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
mod payload;
mod traits;

pub use local::LocalKeySigner;
pub use nip46::{Nip46Signer, Nip46SignerHandle};
pub use nip07::Nip07Signer;
pub use payload::{LocalKeyMaterial, LocalPayload, Nip46Payload, Nip07Payload, SignerPayload};
pub use traits::{Nip04, Nip44, Signer, SignerBackend, SignerError};

// `SignerOp`, `Nip46Rpc`, and `Nip46Transport` are defined in the leaf
// `nmp_signer_iface` crate so `nmp-core` can refer to them without violating
// doctrine **D0**.  Re-exported here so existing downstream paths
// (`nmp_signers::signers::SignerOp`, etc.) keep resolving.
pub use nmp_signer_iface::{Nip46Rpc, Nip46Transport, SignerOp};

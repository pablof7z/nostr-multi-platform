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
mod nip07;
mod nip46;
mod nip55;
mod payload;
mod traits;

pub use local::LocalKeySigner;
pub use nip07::Nip07Signer;
pub use nip46::{Nip46Signer, Nip46SignerHandle};
pub use nip55::{Nip55Connect, Nip55Signer};
// V-01 Stage 3c — the async sign-via-extension entrypoint the wasm runtime
// awaits inside `dispatch_action_async` (its Promise wrapper). The trait
// `Signer::sign()` returns `SignerOp::Pending(rx)` for native-actor-loop
// compatibility; on wasm32 the mpsc receiver cannot be awaited cleanly
// (`recv_timeout` deadlocks the wasm thread; `try_recv` busy-polls — both
// hazards documented on `Nip07Signer::sign`'s docstring). This free function
// returns a real `Future` that yields control to the JS event loop through
// `JsFuture::from(promise).await`.
#[cfg(all(target_arch = "wasm32", feature = "wasm"))]
pub use nip07::wasm::sign_event_via_extension;
pub use payload::{
    LocalKeyMaterial, LocalPayload, Nip07Payload, Nip46Payload, Nip55Payload, SignerPayload,
};
pub use traits::{Nip04, Nip44, Signer, SignerBackend};

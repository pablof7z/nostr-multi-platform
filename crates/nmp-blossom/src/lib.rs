//! `nmp-blossom` — Blossom (BUD-02) blob uploads as an NMP protocol crate.
//!
//! A Layer-4 protocol crate, structurally identical to `nmp-nip57`: it owns the
//! full Build → Sign → Transport pipeline for a Blossom upload and exposes it
//! as a single typed action. Apps dispatch `nmp.blossom.upload` and read a blob
//! descriptor from the `action_results[correlation_id].result` projection — no
//! HTTP, base64, header construction, or sign-for-return in app code.
//!
//! - **`auth`** — pure kind:24242 authorization builder (5-minute TTL) + the
//!   `Authorization: Nostr <base64>` header value. No I/O.
//! - **`upload`** — [`BlossomUploadCommand`] (`ProtocolCommand`): the two-leg
//!   worker (hash+build → sign hop → multi-server PUT) and result aggregation.
//!   `upload::http` is the BUD-02 streaming PUT + descriptor parse.
//! - **`action`** — [`UploadAction`] (`ActionModule`, `nmp.blossom.upload`).
//!
//! Signing goes through `nmp-core`'s generic, backend-transparent
//! `SignEventForAccount` port (ADR-0043 Decision 2): local nsec and NIP-46
//! bunker accounts are both supported, transparently. `nmp-core` learns no
//! Blossom noun and imports no HTTP crate (D0); the kind constant lives in the
//! Layer-0 `nmp-kinds` registry.

pub mod action;
pub mod auth;
pub mod kinds;
pub mod upload;

pub use action::{UploadAction, UploadInput};
pub use auth::{authorization_header_value, build_upload_auth, AUTH_TTL_SECS};
pub use kinds::KIND_BLOSSOM_AUTH;
pub use upload::http::BlobDescriptor;
pub use upload::BlossomUploadCommand;

/// Register the Blossom action(s) on an [`ActionRegistrar`]
/// (`nmp_core::substrate`). Mirrors `nmp_nip57::register_actions`.
///
/// [`ActionRegistrar`]: nmp_core::substrate::ActionRegistrar
pub fn register_actions(app: &mut impl nmp_core::substrate::ActionRegistrar) {
    app.register_action(UploadAction);
}

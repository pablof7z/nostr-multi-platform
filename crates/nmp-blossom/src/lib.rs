//! `nmp-blossom` — Blossom (BUD-02) blob uploads as an NMP protocol crate.
//!
//! A Layer-4 protocol crate, structurally identical to `nmp-nip57`: it owns the
//! full Build → Sign → Transport pipeline for a Blossom upload and exposes it
//! as a single typed action. Apps dispatch `nmp.blossom.upload`, retain the
//! returned `correlation_id`, and read the blob descriptor (`url`, `sha256`, …)
//! from the `action_results[correlation_id].result` projection on a later tick —
//! the canonical completion carrier (ADR-0071 Decision 4, issue #1648). Use
//! [`parse_upload_completion`] to decode the terminal `result` body. Do **not**
//! use `register_action_result_observer` for completion — that push channel
//! fires on accept/enqueue only. No HTTP, base64, header construction, or
//! sign-for-return in app code.
//!
//! - **`auth`** — pure kind:24242 authorization builder (5-minute TTL) + the
//!   `Authorization: Nostr <base64>` header value. No I/O.
//! - **`upload`** — [`BlossomUploadCommand`] (`ProtocolCommand`): the two-leg
//!   worker (hash+build → sign hop → multi-server PUT) and result aggregation.
//!   `upload::http` is the BUD-02 streaming PUT + descriptor parse.
//! - **`action`** — [`UploadAction`] (`ActionModule`, `nmp.blossom.upload`).
//!
//! Signing goes through `nmp-core`'s generic, backend-transparent
//! `SignEventForAccount` port (ADR-0071 Decision 2): local nsec and NIP-46
//! bunker accounts are both supported, transparently. `nmp-core` learns no
//! Blossom noun and imports no HTTP crate (D0); the kind constant lives in the
//! Layer-0 `nmp-kinds` registry.

pub mod action;
pub mod auth;
pub mod kinds;
pub mod result;
pub mod upload;
// ADR-0071 / S9 (#1747) — typed FlatBuffers payload codec (`ActionPayload`
// impl for `UploadInput`).
mod wire;

pub use action::{UploadAction, UploadInput};
pub use auth::{authorization_header_value, build_upload_auth, AUTH_TTL_SECS};
pub use kinds::KIND_BLOSSOM_AUTH;
pub use result::{
    completion_url_sha256, parse_upload_completion, ServerUploadOutcome, UploadCompletion,
};
pub use upload::http::BlobDescriptor;
pub use upload::BlossomUploadCommand;

#[derive(Clone, Debug, Default)]
pub struct Config {}

#[derive(Clone, Debug, Default)]
pub struct Handles {}

/// Register the Blossom action(s) on an [`ActionRegistrar`]
/// (`nmp_core::substrate`).
///
/// [`ActionRegistrar`]: nmp_core::substrate::ActionRegistrar
pub fn register(
    app: &mut impl nmp_core::substrate::ActionRegistrar,
    _config: Config,
) -> Result<Handles, nmp_core::substrate::RegistrationError> {
    app.register_action(UploadAction)?;
    Ok(Handles {})
}

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;

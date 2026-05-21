//! Marmot (MLS-over-Nostr) per-app C-ABI shell for Chirp.
//!
//! All business logic now lives in `nmp_marmot::projection`
//! (`ops` / `state` / `payload` / `publish` / `tap`). Chirp retains ONLY
//! the six `#[no_mangle] extern "C"` symbols in [`ffi`] — proof that the
//! NMP crates are reusable from any host. No MLS / MDK type crosses this
//! C-ABI (`group_id` is hex, errors are strings).
//!
//! ## Doctrine
//!
//! * **D0** — `nmp-core` never depends on `nmp-marmot`; this crate is the
//!   composition point (ADR-0009).
//! * **D6** — every FFI symbol degrades silently (null / `{"ok":false}`),
//!   never panics across the boundary.

pub mod credential_store;
pub mod fetch;
pub mod ffi;
pub mod identity;

//! Response mapping for `Nip55Signer::sign`.
//!
//! The NIP-55 `sign_event` response carries the same signed-event JSON shape
//! as NIP-46, so this module re-uses the NIP-46 mapper verbatim.  It is a thin
//! re-export wrapper so `nip55/mod.rs` can say
//! `use crate::signers::nip55::mapper::map_response_to_event` symmetrically
//! to `nip46/mapper.rs`.
//!
//! The trust model is identical to NIP-46 (codex review #3, 9944bed):
//! the remote-sourced `id` / `sig` are recomputed and verified by
//! `nostr::Event::verify()` before being accepted; the response pubkey is
//! cross-checked against the cached `user_pubkey`.

// Re-export the shared NIP-46 mapper — the signed-event JSON body is the same
// for both transport paths.
pub(crate) use crate::signers::nip46::mapper::map_response_to_event;

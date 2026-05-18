//! NIP-22 kinds owned by this crate.
//!
//! Only standalone comments (kind 1111 without an `h` tag). Kind 1111 events
//! that carry an `h` tag belong to `nmp-nip29` — the `(kind, h-tag-present)`
//! D4 discriminator from `kind-wrappers.md` §6.

/// NIP-22 generic comment.
pub const KIND_COMMENT: u32 = 1111;

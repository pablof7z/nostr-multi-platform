//! `nmp-nip51` — NIP-51 lists as an NMP protocol crate.
//!
//! Implements the design recommendation in `docs/design/kind-wrappers.md` §3:
//! the read side is a pure `try_from_event` decoder; the write side is a set
//! of per-list-type builders that produce an `UnsignedEvent`. Read + write
//! share no mutable state — there is no NDK-style `list.add(...)` setter, which
//! would violate D4 (single writer per fact).
//!
//! ## Scope — six kinds, one classified record
//!
//! This crate owns exactly the six kinds the task brief enumerates:
//!
//! | Kind  | Shape                    | `ListKind`    |
//! |-------|--------------------------|---------------|
//! | 10000 | replaceable              | `Mute`        |
//! | 10002 | replaceable              | `RelayList`   |
//! | 10003 | replaceable              | `Bookmark`    |
//! | 30000 | parameterized (`d` reqd) | `FollowSet`   |
//! | 30002 | parameterized (`d` reqd) | `RelaySet`    |
//! | 30003 | parameterized (`d` reqd) | `BookmarkSet` |
//!
//! Per anti-pattern §9 #3 (no one-class-many-kinds with kind-discriminated
//! getters) the decoder returns ONE uniform [`ListRecord`] carrying a
//! [`ListKind`] classifier. Kind 10002 overlaps NIP-65 deliberately per the
//! brief — this crate's decoder is a read-side projection only and never feeds
//! outbox routing (the kernel-resident NIP-65 path remains authoritative).
//!
//! ## Encrypted private entries
//!
//! NIP-51 private entries are NIP-04-encrypted JSON in the event `.content`.
//! Decoding here is pure (no signer, no I/O), so the ciphertext is preserved
//! verbatim into [`ListRecord::encrypted_payload`] and **never decrypted** —
//! decryption is a downstream actor concern. Builders emit only public tags and
//! leave `content` empty for the same reason.
//!
//! ## Module layout
//!
//! - [`kinds`] — the six kind constants + `is_parameterized`.
//! - [`decode`] — `ListRecord` / `ListKind` / `ListItems` + `try_from_event`.
//! - [`build`] — per-type builders (`MuteList`, `BookmarkList`, `RelayList`,
//!   `FollowSet`, `RelaySet`, `BookmarkSet`) → `UnsignedEvent`.
//! - [`domain`] — `Nip51Domain: DomainModule`; composite `(author, kind,
//!   d_tag)` reverse indexes; `decode_and_route` with NIP-33 / replaceable
//!   supersession.
//! - [`view`] — `ListView` + `ListDetailView`.

pub mod build;
pub mod decode;
pub mod domain;
pub mod kinds;
pub mod view;

pub use build::{
    BookmarkList, BookmarkSet, FollowSet, ListBuilder, MuteList, Nip51BuildError, RelayList,
    RelaySet,
};
pub use decode::{
    try_from_event, try_from_kernel_event, ListItems, ListKind, ListRecord, RelayEntry,
};
pub use domain::{
    decode_and_route, get, list_all, list_by_author, list_by_author_kind, Nip51Domain, NAMESPACE,
};
pub use kinds::{
    is_parameterized, ALL_KINDS, KIND_BOOKMARK_LIST, KIND_BOOKMARK_SETS, KIND_FOLLOW_SETS,
    KIND_MUTE_LIST, KIND_RELAY_LIST, KIND_RELAY_SETS,
};
pub use view::{
    ListAccumulator, ListDetailPayload, ListDetailSpec, ListDetailView, ListListPayload,
    ListListSpec, ListView, ListViewDelta, PublicKey,
};

use nmp_core::substrate::ModuleRegistry;

/// Register every module produced by `nmp-nip51` into a kernel
/// `ModuleRegistry`. Called by per-app generated code (`nmp-codegen`).
pub fn register(registry: &mut ModuleRegistry) {
    registry.register_domain::<Nip51Domain>();
    view::register_all(registry);
}

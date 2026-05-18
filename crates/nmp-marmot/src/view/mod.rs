//! 3 `ViewModule` impls per `docs/plan/marmot-mls.md` §Step 1.
//!
//! `GroupList`, `GroupMessages`, `MemberList`. All group-scoped views are
//! relay-pinned to the group relay via `InterestShape::relay_pin` (ADR-0012);
//! the interest helpers in [`crate::interest`] carry the pin.
//!
//! ## Projection scope (this milestone)
//!
//! Marmot kind:445 events are MLS-encrypted on the wire — the kernel's raw
//! ingest path sees only ciphertext. The authoritative decrypted projection
//! comes from MDK via [`crate::service`] (`get_groups` / `get_messages` /
//! `get_members`). These view modules ship correct trait signatures + correct
//! relay-pinned dependency declarations; the decrypted snapshot is filled by
//! the service/actor layer (same Step-0 scope-cut as `nmp-nip29`'s views).

mod shared;
mod views;

pub use shared::{EventAccumulator, EventAccumulatorDelta};
pub use views::{
    GroupListEntry, GroupListPayload, GroupListSpec, GroupListView, GroupMessageEntry,
    GroupMessagesPayload, GroupMessagesSpec, GroupMessagesView, MemberEntry, MemberListPayload,
    MemberListSpec, MemberListView,
};

use nmp_core::substrate::ModuleRegistry;

/// Register all 3 `ViewModule` impls into a kernel `ModuleRegistry`.
pub fn register_all(registry: &mut ModuleRegistry) {
    registry.register_view::<GroupListView>();
    registry.register_view::<GroupMessagesView>();
    registry.register_view::<MemberListView>();
}

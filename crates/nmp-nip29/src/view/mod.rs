//! 7 `ViewModule` impls per `docs/design/nip29-crate.md` §3.2.
//!
//! Each view declares `LogicalInterest`s via `dependencies()` that the M2
//! compiler turns into per-relay REQs. All NIP-29 views are **host-pinned**
//! via the `relay_pin` field on `InterestShape` (routing.md §3); the compiler's
//! Case E + lattice Rule 9 do the work.
//!
//! ## Projection scope (M11.5 Step 0 scope-cut)
//!
//! The view modules ship correct trait signatures + correct dependency
//! declarations + correct host-pinned routing. Their projection logic
//! (in-state accumulation, delta emission, snapshot rendering) is intentionally
//! minimal — enough to validate the substrate contract holds, with the rich
//! UI-driven projection (composer integration, hydrated profile joins, etc.)
//! landing as Step 5 of M11.5 alongside the Swift wiring.
//!
//! Each view's `State` is a thin `Vec<KernelEvent>` accumulator; `snapshot`
//! returns the events sorted by `created_at`. Cross-protocol hydration
//! (`HydratedGroupChat` joining `nmp-nip01::Profile`) lives in the app crate
//! (`nmp-highlighter-core`), not here, per the M11.5 exit-gate
//! protocol-crate-isolation rule.

mod chat;
mod explorer;
mod home;
mod joined;
mod members;
mod shared;

pub use chat::{
    ArtifactsPayload, ArtifactsSpec, ChatPayload, ChatSpec, DiscussionsPayload, DiscussionsSpec,
    GroupArtifactsView, GroupChatView, GroupDiscussionsView,
};
pub use explorer::{ExplorerPayload, ExplorerSpec, GroupExplorerView};
pub use home::{GroupHomeView, HomePayload, HomeSpec};
pub use joined::{JoinedGroupsView, JoinedPayload, JoinedSpec};
pub use members::{GroupMembersView, MembersPayload, MembersSpec};
pub use shared::{EventAccumulator, EventAccumulatorDelta};

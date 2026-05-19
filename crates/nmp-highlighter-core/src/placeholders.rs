//! Placeholders for the Step 3 + Step 5 module surfaces.
//!
//! These are *typed marker* structs / enums whose presence on master means the
//! crate's surface is shaped correctly even though the full impls land later.
//!
//! Surface map (per `docs/plan/m11.5-highlighter.md` §Step 3):
//!
//! - `ReadsFeed` — ViewModule (articles + podcasts + books interleave)
//! - `FollowingHighlights` — ViewModule (merge of nmp-nip84 + nmp-nip29::GroupHighlight)
//! - `SearchIndex` — ViewModule (cross-entity search)
//! - `Recommendations` — ViewModule (rooms / follows)
//! - `CaptureFlow` — ActionModule family (URL / PDF / book / podcast / share-extension)
//! - `Feedback` — ActionModule (kind:1 + kind:513 dogfood loop)
//! - `WebMetadata` — capability bridge (OpenGraph fetch + cache via HttpCapability)
//! - `IsbnLookup` — capability bridge (book metadata via HttpCapability)
//! - `BookRegistry` — DomainModule + `RecentBooks` ViewModule
//! - `PublishHighlightAndShareToGroup` — composed ActionModule

use serde::{Deserialize, Serialize};

/// Marker that the Highlighter-specific ViewModule + ActionModule surface
/// exists at the boundary. Replaced by real impls in M11.5 Steps 3 + 5.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Step0Scaffold;

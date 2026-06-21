//! Content-render sidecar payloads for timeline cards.
//!
//! These plain serde structs carry the profile/event embed lookup tables a
//! card's content tree references (the `ContentRenderData` sidecar). They are
//! re-exported from [`super`] so existing `crate::timeline_projection::*` paths
//! (e.g. `nmp-nip01`'s `typed_wire` encoder) keep resolving unchanged. Split
//! out of `timeline_projection.rs` to keep that file under the LOC ceiling
//! (AGENTS.md file-size rule).

use std::collections::BTreeMap;

use nmp_content::ContentTreeWire;
use serde::{Deserialize, Serialize};

use crate::profile_display::AuthorDisplay;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ContentRenderData {
    pub profiles: BTreeMap<String, ContentProfileRenderData>,
    pub events: BTreeMap<String, ContentEventRenderData>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ContentProfileRenderData {
    pub pubkey: String,
    pub display: AuthorDisplay,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ContentEventRenderData {
    pub id: String,
    pub author_pubkey: String,
    pub author_display: AuthorDisplay,
    pub kind: u32,
    pub created_at: u64,
    pub content_preview: String,
    pub content_tree: ContentTreeWire,
}

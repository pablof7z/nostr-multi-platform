//! Wire envelope and render context for embedded events.

use serde::{Deserialize, Serialize};

use crate::context::RenderContext;

/// Full envelope for one embedded event that crosses the FFI wire to native.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddedEventEnvelope {
    /// The original nostr: URI string that triggered the embed (nevent1… or naddr1…).
    pub uri: String,
    /// Primary identifier: event id hex for event-addressed refs, or the
    /// "kind:pubkey:d" coordinate string for addressable events.
    pub primary_id: String,
    /// Recursion guard state at the point this embed was encountered.
    pub render_context: RenderContextWire,
    /// The kind-dispatched projection (drives which native renderer is chosen).
    pub projection: super::EmbedKindProjection,
    /// Whether this embed should be collapsed (depth limit, cycle, or unsupported).
    pub collapsed: bool,
    /// Optional machine-readable reason for collapse: "depth_limit" | "cycle" | "unsupported".
    pub collapse_reason: Option<String>,
}

/// Serializable form of [`RenderContext`] for the wire / FFI boundary.
/// `visited` uses hex event id strings (same shape as other wire types).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderContextWire {
    /// Current embed recursion depth.
    pub depth: u8,
    /// Maximum allowed embed recursion depth for this render pass.
    pub max_depth: u8,
    /// Hex event ids already visited in this recursive render path.
    pub visited: Vec<String>,
}

impl From<&RenderContext> for RenderContextWire {
    fn from(ctx: &RenderContext) -> Self {
        Self {
            depth: ctx.depth,
            max_depth: ctx.max_depth,
            visited: ctx.visited.iter().cloned().collect(),
        }
    }
}

impl From<&RenderContextWire> for RenderContext {
    fn from(w: &RenderContextWire) -> Self {
        // Note: SmallVec will be populated from the vec; we accept the heap
        // cost on the wire-to-native boundary because this is infrequent.
        let mut visited = smallvec::SmallVec::new();
        for id in &w.visited {
            visited.push(id.clone());
        }
        Self {
            depth: w.depth,
            max_depth: w.max_depth,
            visited,
        }
    }
}

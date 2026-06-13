//! `GroupDefaultsProjection` — crate-owned defaults for the NIP-29
//! public-group create flow.
//!
//! Unlike [`super::group_chat::GroupChatProjection`] and
//! [`super::discovered::DiscoveredGroupsProjection`], this projection is **not**
//! event-driven: it carries no [`nmp_core::KernelEventObserver`] and observes no
//! kernel events. It is a pure **output** projection that surfaces a constant
//! the framework owns — the suggested host relay URL a host shell pre-fills into
//! its "new public group" form.
//!
//! ## Why this lives in Rust, not the shell
//!
//! Issue #626: the iOS `NewGroupSheet` hardcoded the default relay URL as a
//! compile-time Swift `@State` literal
//! (`"wss://relay.groups.nip29.com"`). That is a NIP-29 protocol fact (a
//! protocol-specific relay URL) baked into a thin shell — a P5 violation
//! (the framework, not the shell, must own protocol complexity) and it could
//! not be changed without a client release.
//!
//! Surfacing it as a snapshot projection keyed `"nmp.nip29.group_defaults"`
//! lets every host shell read the suggested URL off the kernel snapshot, and
//! lets the value move (per-build today, per-kernel-config later) without any
//! shell change. The shell keeps only the editable `TextField` binding.
//!
//! ## D0 compliance
//!
//! The NIP-29 nouns (`group`, the protocol relay URL) live here, in the NIP-29
//! crate — never in `nmp-core` (nip29 nouns are D0-banned there). The
//! projection is registered from [`crate::register::wire_group_defaults`]
//! alongside the other NIP-29 projections.

use serde::{Deserialize, Serialize};

/// The crate-owned default host relay URL for newly created NIP-29 public
/// groups.
///
/// This is the single source of truth for the value issue #626 moved out of
/// the iOS shell. It is sourced into the `"nmp.nip29.group_defaults"` snapshot
/// projection so every host shell reads it identically. Changing the default
/// is a one-line edit here — never a per-shell change.
pub const DEFAULT_PUBLIC_GROUP_RELAY_URL: &str = "wss://relay.groups.nip29.com";

/// The read model surfaced under `"nmp.nip29.group_defaults"`.
///
/// A flat carrier of the create-flow defaults a host shell pre-fills. Today it
/// carries only `suggested_relay_url`; it is a struct (not a bare string) so
/// future defaults (e.g. a suggested-id placeholder) extend it without a new
/// projection key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupDefaultsSnapshot {
    /// Suggested host relay URL for a new public group. A host shell seeds its
    /// editable relay-URL field from this; the user may overwrite it.
    pub suggested_relay_url: String,
}

impl GroupDefaultsSnapshot {
    /// Build the defaults snapshot from the crate-owned constant.
    #[must_use]
    pub fn from_defaults() -> Self {
        Self {
            suggested_relay_url: DEFAULT_PUBLIC_GROUP_RELAY_URL.to_string(),
        }
    }
}

impl Default for GroupDefaultsSnapshot {
    fn default() -> Self {
        Self::from_defaults()
    }
}

/// The output-only projection of NIP-29 create-flow defaults.
///
/// Holds no mutable state and observes no events — its snapshot is a pure
/// function of [`DEFAULT_PUBLIC_GROUP_RELAY_URL`]. Registered as a snapshot
/// projection (plus a typed FlatBuffers sidecar) under
/// `"nmp.nip29.group_defaults"`.
#[derive(Debug, Clone, Default)]
pub struct GroupDefaultsProjection;

impl GroupDefaultsProjection {
    /// Construct the (stateless) defaults projection.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// The typed read model carried on every snapshot.
    #[must_use]
    pub fn snapshot(&self) -> GroupDefaultsSnapshot {
        GroupDefaultsSnapshot::from_defaults()
    }

    /// The generic `serde_json::Value` projection body registered under
    /// `"nmp.nip29.group_defaults"` (the permanent ADR-0037 fallback carried
    /// alongside the typed sidecar).
    #[must_use]
    pub fn snapshot_json(&self) -> serde_json::Value {
        serde_json::to_value(self.snapshot()).unwrap_or(serde_json::Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_sources_the_crate_owned_constant() {
        let snap = GroupDefaultsProjection::new().snapshot();
        assert_eq!(snap.suggested_relay_url, DEFAULT_PUBLIC_GROUP_RELAY_URL);
        // The constant is the documented #626 value — guard against an
        // accidental edit that would silently change every shell's default.
        assert_eq!(snap.suggested_relay_url, "wss://relay.groups.nip29.com");
    }

    #[test]
    fn snapshot_json_carries_suggested_relay_url() {
        let json = GroupDefaultsProjection::new().snapshot_json();
        assert_eq!(
            json.get("suggested_relay_url").and_then(|v| v.as_str()),
            Some(DEFAULT_PUBLIC_GROUP_RELAY_URL)
        );
    }
}

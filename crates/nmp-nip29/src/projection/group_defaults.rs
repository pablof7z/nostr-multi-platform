//! `GroupDefaultsProjection` — app-supplied defaults for the NIP-29
//! public-group create flow.
//!
//! Unlike [`super::group_timeline::GroupTimelineProjection`] and
//! [`super::discovered::DiscoveredGroupsProjection`], this projection is **not**
//! event-driven: it carries no [`nmp_core::ObservedProjectionSink`] and observes no
//! kernel events. It is a pure **output** projection that surfaces the
//! app/operator-supplied suggested host relay URL a host shell pre-fills into
//! its "new public group" form.
//!
//! ## Why this lives in Rust, not the shell
//!
//! Issue #626: the iOS `NewGroupSheet` hardcoded the default relay URL as a
//! compile-time Swift `@State` literal. That was product/operator policy baked
//! into a thin shell — a boundary violation because platform shells render and
//! execute capabilities only.
//!
//! Surfacing it as a snapshot projection keyed `"nmp.nip29.group_defaults"`
//! lets every host shell read the suggested URL off the kernel snapshot while
//! the leaf app Rust config remains the single policy owner. The shell keeps
//! only the editable `TextField` binding.
//!
//! ## D0 compliance
//!
//! The NIP-29 nouns (`group`, group defaults) live here, in the NIP-29 crate —
//! never in `nmp-core` (nip29 nouns are D0-banned there). The relay URL itself
//! is app/operator policy: [`crate::register::wire_group_defaults`] emits an
//! empty suggestion, while leaf apps call
//! [`crate::register::wire_group_defaults_with_relay`].

use serde::{Deserialize, Serialize};

/// The shared-crate default host relay URL for newly created NIP-29 public
/// groups.
///
/// Empty by design: shared NMP crates do not own public relay/operator policy.
/// Leaf apps that want a pre-filled group host relay call
/// [`crate::register::wire_group_defaults_with_relay`].
pub const DEFAULT_PUBLIC_GROUP_RELAY_URL: &str = "";

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
    /// Build the empty shared defaults snapshot.
    #[must_use]
    pub fn from_defaults() -> Self {
        Self::with_suggested_relay_url(DEFAULT_PUBLIC_GROUP_RELAY_URL)
    }

    /// Build the defaults snapshot from an app/operator-supplied relay URL.
    #[must_use]
    pub fn with_suggested_relay_url(url: impl Into<String>) -> Self {
        Self {
            suggested_relay_url: url.into(),
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
/// Holds no mutable state and observes no events — its snapshot is the
/// app-supplied value captured at registration time. Registered as a typed
/// FlatBuffers snapshot projection under `"nmp.nip29.group_defaults"`.
#[derive(Debug, Clone)]
pub struct GroupDefaultsProjection {
    snapshot: GroupDefaultsSnapshot,
}

impl Default for GroupDefaultsProjection {
    fn default() -> Self {
        Self::new()
    }
}

impl GroupDefaultsProjection {
    /// Construct the defaults projection with no suggested relay.
    #[must_use]
    pub fn new() -> Self {
        Self::with_snapshot(GroupDefaultsSnapshot::from_defaults())
    }

    /// Construct the defaults projection from an app/operator-supplied relay.
    #[must_use]
    pub fn with_suggested_relay_url(url: impl Into<String>) -> Self {
        Self::with_snapshot(GroupDefaultsSnapshot::with_suggested_relay_url(url))
    }

    /// Construct the defaults projection from an explicit snapshot.
    #[must_use]
    pub fn with_snapshot(snapshot: GroupDefaultsSnapshot) -> Self {
        Self { snapshot }
    }

    /// The typed read model carried on every snapshot.
    #[must_use]
    pub fn snapshot(&self) -> GroupDefaultsSnapshot {
        self.snapshot.clone()
    }

    /// Serde JSON mirror of the typed read model.
    ///
    /// `wire_group_defaults*` does not register this as a generic projection;
    /// it exists for serialization parity tests and callers that need the JSON
    /// shape explicitly.
    #[must_use]
    pub fn snapshot_json(&self) -> serde_json::Value {
        serde_json::to_value(self.snapshot()).unwrap_or(serde_json::Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_snapshot_has_no_shared_relay_url() {
        let snap = GroupDefaultsProjection::new().snapshot();
        assert_eq!(snap.suggested_relay_url, DEFAULT_PUBLIC_GROUP_RELAY_URL);
        assert!(
            snap.suggested_relay_url.is_empty(),
            "shared NIP-29 defaults must not name a public relay"
        );
    }

    #[test]
    fn snapshot_carries_app_supplied_relay_url() {
        let snap =
            GroupDefaultsProjection::with_suggested_relay_url("wss://groups.example").snapshot();
        assert_eq!(snap.suggested_relay_url, "wss://groups.example");
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

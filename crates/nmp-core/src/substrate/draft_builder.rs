//! Composition-registered draft builders.
//!
//! Core owns the publish intent doorway and signing/routing machinery, but
//! protocol crates own artifact grammar. `DraftBuilderRegistry` is the narrow
//! seam between them: reducers resolve an intent to an unsigned event without
//! naming kind-specific tag or content rules.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use nmp_signer_iface::UnsignedEvent;
use nmp_store::EventStore;

/// Publish intents whose wire artifact is provided by a protocol builder.
#[derive(Clone, Debug, PartialEq)]
pub enum DraftIntent {
    /// NIP-01 reply intent. NIP-10 tag grammar belongs to the registered builder.
    Reply {
        content: String,
        reply_to_event_id: String,
    },
    /// NIP-01 profile metadata intent. Kind/content grammar belongs to the
    /// registered builder.
    Profile {
        fields: serde_json::Map<String, serde_json::Value>,
    },
}

impl DraftIntent {
    #[must_use]
    pub fn kind(&self) -> DraftIntentKind {
        match self {
            Self::Reply { .. } => DraftIntentKind::Reply,
            Self::Profile { .. } => DraftIntentKind::Profile,
        }
    }
}

/// Registry key for a draft intent.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DraftIntentKind {
    Reply,
    Profile,
}

/// Immutable inputs a protocol builder may use to materialize an unsigned event.
pub struct DraftBuildContext<'a> {
    pub event_store: &'a dyn EventStore,
    pub author_pubkey: &'a str,
    pub created_at: u64,
}

/// Structured builder failure. Reducers surface the reason through existing D6
/// error channels; it never crosses FFI as a panic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DraftBuildError {
    reason: String,
}

impl DraftBuildError {
    #[must_use]
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }

    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl core::fmt::Display for DraftBuildError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.reason)
    }
}

impl std::error::Error for DraftBuildError {}

/// Protocol-owned artifact builder.
pub trait DraftBuilder: Send + Sync {
    fn build(
        &self,
        intent: &DraftIntent,
        ctx: DraftBuildContext<'_>,
    ) -> Result<UnsignedEvent, DraftBuildError>;
}

/// Cloneable composition registry for protocol draft builders.
#[derive(Default)]
pub struct DraftBuilderRegistry {
    builders: RwLock<BTreeMap<DraftIntentKind, Arc<dyn DraftBuilder>>>,
}

impl DraftBuilderRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, kind: DraftIntentKind, builder: Arc<dyn DraftBuilder>) {
        if let Ok(mut guard) = self.builders.write() {
            guard.insert(kind, builder);
        }
    }

    pub fn build(
        &self,
        intent: &DraftIntent,
        ctx: DraftBuildContext<'_>,
    ) -> Result<UnsignedEvent, DraftBuildError> {
        let builder = self
            .builders
            .read()
            .ok()
            .and_then(|guard| guard.get(&intent.kind()).cloned())
            .ok_or_else(|| {
                DraftBuildError::new(format!("draft_builder_missing: {:?}", intent.kind()))
            })?;
        builder.build(intent, ctx)
    }
}

/// Register draft builders by intent.
pub trait DraftBuilderRegistrar {
    fn register_draft_builder(&self, kind: DraftIntentKind, builder: Arc<dyn DraftBuilder>);
}

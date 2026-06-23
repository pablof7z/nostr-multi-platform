//! [`InputScopeId`] — the namespaced identity of a registered input scope.

use serde::{Deserialize, Serialize};

/// Stable identity of a registered input scope.
///
/// `namespace` + `name` form a two-part, human-readable label (the shared
/// namespaced-label vocabulary convention used across the substrate, e.g.
/// `nostr.ref`, `nip50.profiles`). Unlike the FTS [`crate::store::SearchScopeId`]
/// — which hashes a `&'static str` to a numeric discriminant because it keys a
/// storage index — input-scope ids are owned strings: recognizers may be
/// registered at runtime by an app with a dynamic id, and the registry compares
/// by full `(namespace, name)` value.
///
/// Synthetic always-allowed scope: [`InputScopeId::NOSTR_REF`] (`nostr.ref`) is
/// the id a NIP-19/21 reference recognizer claims. The resolver treats it as
/// always requested when a valid ref is present, but a ref whose *target class*
/// is excluded by the app's requested scopes is still refused
/// (`DisallowedScope`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct InputScopeId {
    pub namespace: String,
    pub name: String,
}

impl InputScopeId {
    /// Construct from owned namespace + name.
    #[must_use]
    pub fn new(namespace: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            name: name.into(),
        }
    }

    /// Render as the canonical `namespace.name` label (diagnostics / ledger key).
    #[must_use]
    pub fn label(&self) -> String {
        format!("{}.{}", self.namespace, self.name)
    }

    /// Namespace of the synthetic, always-allowed direct-reference scope. A
    /// NIP-19/21 reference recognizer claims `nostr.ref`.
    pub const NOSTR_REF_NAMESPACE: &'static str = "nostr";
    /// Name of the synthetic direct-reference scope.
    pub const NOSTR_REF_NAME: &'static str = "ref";

    /// The synthetic, always-allowed direct-reference scope id (`nostr.ref`).
    #[must_use]
    pub fn nostr_ref() -> Self {
        Self::new(Self::NOSTR_REF_NAMESPACE, Self::NOSTR_REF_NAME)
    }
}

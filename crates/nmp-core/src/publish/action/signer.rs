use serde::{Deserialize, Serialize};

/// Why a publish action is allowed to select a registered signer directly.
///
/// The selector is still resolved by the actor-owned signer roster. Provenance
/// is app-facing intent, not authority: an unknown pubkey still fails in the
/// signing stage with the structured "no signer for account ..." failure.
#[derive(Copy, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PublishSignerProvenance {
    AppManaged,
    UserSelected,
    ProtocolPinned,
    Diagnostic,
}

impl Default for PublishSignerProvenance {
    fn default() -> Self {
        Self::AppManaged
    }
}

impl PublishSignerProvenance {
    #[must_use]
    pub fn wire_token(self) -> &'static str {
        match self {
            Self::AppManaged => "app_managed",
            Self::UserSelected => "user_selected",
            Self::ProtocolPinned => "protocol_pinned",
            Self::Diagnostic => "diagnostic",
        }
    }

    #[must_use]
    pub fn from_wire_token(token: &str) -> Option<Self> {
        Some(match token {
            "app_managed" => Self::AppManaged,
            "user_selected" => Self::UserSelected,
            "protocol_pinned" => Self::ProtocolPinned,
            "diagnostic" => Self::Diagnostic,
            _ => return None,
        })
    }
}

/// Signer selected for a sign-and-publish action.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PublishSigner {
    /// Resolve and sign with the active account at actor execution time.
    Active,
    /// Resolve and sign with a registered roster signer by pubkey.
    Registered {
        pubkey: String,
        provenance: PublishSignerProvenance,
    },
}

impl Default for PublishSigner {
    fn default() -> Self {
        Self::Active
    }
}

impl PublishSigner {
    #[must_use]
    pub fn registered(pubkey: String, provenance: PublishSignerProvenance) -> Self {
        Self::Registered { pubkey, provenance }
    }

    #[must_use]
    pub(crate) fn signer_pubkey(&self) -> Option<String> {
        match self {
            Self::Active => None,
            Self::Registered { pubkey, .. } => Some(pubkey.clone()),
        }
    }
}

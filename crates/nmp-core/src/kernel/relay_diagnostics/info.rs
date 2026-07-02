//! Relay information DTO for relay diagnostics.

use serde::Serialize;

/// Relay-information document, projected for the diagnostics surface (ADR-0072).
///
/// A field-for-field surface of the substrate-generic
/// [`crate::substrate::RelayInfoDoc`]. Carried on the relay row so shells
/// render relay name, icon, and capabilities directly.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(in crate::kernel) struct RelayDiagnosticsInfo {
    /// Operator-chosen display name, when advertised.
    pub(in crate::kernel) name: Option<String>,
    /// Human-readable description / "about" text.
    pub(in crate::kernel) description: Option<String>,
    /// Relay icon URL.
    pub(in crate::kernel) icon: Option<String>,
    /// Operator administrative public key (hex).
    pub(in crate::kernel) pubkey: Option<String>,
    /// Operator contact (email / URL / nostr address).
    pub(in crate::kernel) contact: Option<String>,
    /// Relay software identifier.
    pub(in crate::kernel) software: Option<String>,
    /// Relay software version.
    pub(in crate::kernel) version: Option<String>,
    /// Protocol (NIP) numbers the relay advertises support for.
    pub(in crate::kernel) supported_nips: Vec<u32>,
    /// `limitation.payment_required`.
    pub(in crate::kernel) payment_required: Option<bool>,
    /// `limitation.auth_required`.
    pub(in crate::kernel) auth_required: Option<bool>,
    /// `limitation.restricted_writes`.
    pub(in crate::kernel) restricted_writes: Option<bool>,
}

impl RelayDiagnosticsInfo {
    pub(super) fn from_doc(doc: &crate::substrate::RelayInfoDoc) -> Self {
        Self {
            name: doc.name.clone(),
            description: doc.description.clone(),
            icon: doc.icon.clone(),
            pubkey: doc.pubkey.clone(),
            contact: doc.contact.clone(),
            software: doc.software.clone(),
            version: doc.version.clone(),
            supported_nips: doc.supported_nips.clone(),
            payment_required: doc.limitation_payment_required,
            auth_required: doc.limitation_auth_required,
            restricted_writes: doc.limitation_restricted_writes,
        }
    }
}

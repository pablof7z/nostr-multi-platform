//! Positive ownership descriptors for NMP crates.
//!
//! Each crate declares what it owns in code. Empty `claims` are valid: they mean
//! the crate exists to compose, verify, or adapt other owners without claiming a
//! protected semantic surface of its own.

mod macros;

/// One crate's ownership descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrateOwnershipDescriptor {
    /// Stable owner id used in provenance and reports.
    pub owner_id: &'static str,
    /// Cargo package name.
    pub crate_name: &'static str,
    /// One or two lines describing the crate's responsibility.
    pub summary: &'static str,
    /// Positive ownership claims. Absence means no ownership.
    pub claims: &'static [OwnershipClaim],
    /// Boundary notes attached to claims. Notes never grant or deny ownership.
    pub notes: &'static [OwnershipNote],
}

/// One positive ownership claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnershipClaim {
    /// `artifact`, `envelope`, `mechanism`, `namespace`, `schema`, etc.
    pub claim_type: &'static str,
    /// Stable claim id, unique under the owner.
    pub id: &'static str,
    /// Whether another crate may claim the same scoped surface.
    pub exclusive: bool,
    /// Machine-readable scope kind: `kind`, `tag`, `field`, `action`, ...
    pub scope_kind: &'static str,
    /// Machine-readable scope value.
    pub scope_value: &'static str,
    /// Optional collision context. Use this when a wire token is not globally
    /// exclusive, such as a tag string whose semantics are shape-specific.
    pub context: &'static str,
    /// Human-readable responsibilities covered by this claim.
    pub owns: &'static [&'static str],
}

/// A boundary note attached to a claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnershipNote {
    /// Claim id the note explains.
    pub claim: &'static str,
    /// Explanatory text. This is not a claim.
    pub text: &'static str,
}

/// Linker-visible marker emitted for each exclusive ownership scope.
///
/// The marker's exported symbol name is derived from the claim type, scope
/// kind, scope value, and context. If two linked crates claim the same
/// exclusive scope, they export the same symbol and the binary fails to link.
#[repr(C)]
pub struct ExclusiveClaimSymbol {
    /// Stable owner id that emitted the symbol.
    pub owner_id: &'static str,
    /// Stable claim id that emitted the symbol.
    pub claim_id: &'static str,
}

impl ExclusiveClaimSymbol {
    /// Build a linker collision marker.
    #[must_use]
    pub const fn new(owner_id: &'static str, claim_id: &'static str) -> Self {
        Self { owner_id, claim_id }
    }
}

/// Provenance proving which owner minted an artifact draft.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtifactProvenance {
    /// Stable owner id.
    pub owner_id: &'static str,
    /// Stable artifact claim id.
    pub claim_id: &'static str,
}

impl ArtifactProvenance {
    /// Build an artifact provenance token.
    #[must_use]
    pub const fn new(owner_id: &'static str, claim_id: &'static str) -> Self {
        Self { owner_id, claim_id }
    }
}

/// Provenance proving which owner injected an envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvelopeProvenance {
    /// Stable owner id.
    pub owner_id: &'static str,
    /// Stable envelope claim id.
    pub claim_id: &'static str,
}

impl EnvelopeProvenance {
    /// Build an envelope provenance token.
    #[must_use]
    pub const fn new(owner_id: &'static str, claim_id: &'static str) -> Self {
        Self { owner_id, claim_id }
    }
}

/// Provenance carried by an unsigned event draft before signing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventOwnershipProvenance {
    /// Artifact owner, when the event kind/shape is protected.
    pub artifact: Option<ArtifactProvenance>,
    /// Envelope owners that wrapped the artifact.
    pub envelopes: &'static [EnvelopeProvenance],
}

impl EventOwnershipProvenance {
    /// Build event ownership provenance.
    #[must_use]
    pub const fn new(
        artifact: Option<ArtifactProvenance>,
        envelopes: &'static [EnvelopeProvenance],
    ) -> Self {
        Self {
            artifact,
            envelopes,
        }
    }

    fn has_artifact(self, owner_id: &str, claim_id: &str) -> bool {
        self.artifact
            .is_some_and(|p| p.owner_id == owner_id && p.claim_id == claim_id)
    }

    fn has_envelope(self, owner_id: &str, claim_id: &str) -> bool {
        self.envelopes
            .iter()
            .any(|p| p.owner_id == owner_id && p.claim_id == claim_id)
    }
}

/// Event draft minted by an ownership claim before signing.
///
/// The event payload is generic so `nmp-ownership` stays below signer/core
/// crates in the dependency graph. Protocol crates commonly use
/// `OwnedEventDraft<nmp_signer_iface::UnsignedEvent>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedEventDraft<E> {
    event: E,
    ownership: EventOwnershipProvenance,
}

impl<E> OwnedEventDraft<E> {
    /// Create an owner-certified event draft.
    #[must_use]
    pub fn new(event: E, ownership: EventOwnershipProvenance) -> Self {
        Self { event, ownership }
    }

    /// Borrow the unsigned event payload.
    #[must_use]
    pub fn event(&self) -> &E {
        &self.event
    }

    /// Borrow the ownership provenance.
    #[must_use]
    pub fn ownership(&self) -> EventOwnershipProvenance {
        self.ownership
    }

    /// Consume the draft into its event payload and ownership provenance.
    #[must_use]
    pub fn into_parts(self) -> (E, EventOwnershipProvenance) {
        (self.event, self.ownership)
    }

    /// Transform the event payload while preserving ownership provenance.
    #[must_use]
    pub fn map_event<T>(self, f: impl FnOnce(E) -> T) -> OwnedEventDraft<T> {
        let (event, ownership) = self.into_parts();
        OwnedEventDraft::new(f(event), ownership)
    }

    /// Return a draft with replacement ownership provenance.
    #[must_use]
    pub fn with_ownership(self, ownership: EventOwnershipProvenance) -> Self {
        Self {
            event: self.event,
            ownership,
        }
    }
}

/// An event draft after a contextual owner has added envelope provenance.
pub type EnvelopedEventDraft<E> = OwnedEventDraft<E>;

/// Error raised when a publishable event lacks required ownership provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishOwnershipError {
    reason: String,
}

impl PublishOwnershipError {
    fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl core::fmt::Display for PublishOwnershipError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.reason)
    }
}

impl std::error::Error for PublishOwnershipError {}

/// Validate protected publish ownership before signing.
///
/// This intentionally validates positive provenance. Absence means no owner
/// proof, so protected artifacts/envelopes fail closed.
pub fn validate_publish_ownership(
    kind: u32,
    tags: &[Vec<String>],
    provenance: Option<EventOwnershipProvenance>,
    is_group_host_pin: bool,
) -> Result<(), PublishOwnershipError> {
    let proof = provenance;
    if kind == 7 {
        require_artifact(
            proof,
            "nmp.nip25",
            "nostr.kind.7.reaction",
            "kind:7 reaction events must be built by nmp-nip25",
        )?;
    }
    // All kind:5 deletion events must carry nmp-nip09 artifact provenance
    // (ADR-0074). The generic deletion owner is nmp-nip09 regardless of what
    // the caller is deleting (reactions, posts, or anything else). Non-exclusive
    // intent claims (e.g. nmp-nip25's retraction intent) are declaration-only;
    // the publish gate gates on the exclusive artifact claim.
    if kind == 5 {
        require_artifact(
            proof,
            "nmp.nip09",
            "nostr.kind.5.deletion",
            "kind:5 deletion events must be built by nmp-nip09",
        )?;
    }
    if matches!(kind, 39000..=39003) {
        require_artifact(
            proof,
            "nmp.nip29",
            "nostr.nip29.group_metadata",
            "NIP-29 group metadata events must be built by nmp-nip29",
        )?;
    }
    if matches!(
        kind,
        9000 | 9001 | 9002 | 9005 | 9007 | 9008 | 9009 | 9021 | 9022
    ) {
        require_artifact(
            proof,
            "nmp.nip29",
            "nostr.nip29.group_management",
            "NIP-29 group management events must be built by nmp-nip29",
        )?;
    }
    if has_tag(tags, "h") {
        require_envelope(
            proof,
            "nmp.nip29",
            "nostr.nip29.group_envelope",
            "events carrying tag h must be wrapped by the NIP-29 group envelope owner",
        )?;
        if !is_group_host_pin {
            return Err(PublishOwnershipError::new(
                "events carrying tag h must publish through a group host relay pin",
            ));
        }
    }
    if has_tag(tags, "previous") {
        require_envelope(
            proof,
            "nmp.nip29",
            "nostr.nip29.previous_chain",
            "events carrying tag previous must be wrapped by the NIP-29 previous-chain owner",
        )?;
    }
    Ok(())
}

fn require_artifact(
    provenance: Option<EventOwnershipProvenance>,
    owner_id: &'static str,
    claim_id: &'static str,
    reason: &'static str,
) -> Result<(), PublishOwnershipError> {
    if provenance.is_some_and(|p| p.has_artifact(owner_id, claim_id)) {
        Ok(())
    } else {
        Err(PublishOwnershipError::new(reason))
    }
}

fn require_envelope(
    provenance: Option<EventOwnershipProvenance>,
    owner_id: &'static str,
    claim_id: &'static str,
    reason: &'static str,
) -> Result<(), PublishOwnershipError> {
    if provenance.is_some_and(|p| p.has_envelope(owner_id, claim_id)) {
        Ok(())
    } else {
        Err(PublishOwnershipError::new(reason))
    }
}

fn has_tag(tags: &[Vec<String>], name: &str) -> bool {
    tags.iter()
        .any(|tag| tag.first().is_some_and(|t| t == name))
}

#[cfg(test)]
mod tests {
    use crate::{
        validate_publish_ownership, ArtifactProvenance, CrateOwnershipDescriptor,
        EnvelopeProvenance, EventOwnershipProvenance, OwnedEventDraft, OwnershipClaim,
        OwnershipNote,
    };

    #[test]
    fn descriptor_is_typed() {
        const CLAIMS: &[OwnershipClaim] = &[OwnershipClaim {
            claim_type: "artifact",
            id: "nostr.kind.7.reaction",
            exclusive: true,
            scope_kind: "kind",
            scope_value: "7",
            context: "",
            owns: &["reaction construction"],
        }];
        const NOTES: &[OwnershipNote] = &[OwnershipNote {
            claim: "nostr.kind.7.reaction",
            text: "A note does not grant ownership.",
        }];
        const DESCRIPTOR: CrateOwnershipDescriptor = CrateOwnershipDescriptor {
            owner_id: "nmp.test",
            crate_name: "nmp-test",
            summary: "Test descriptor.",
            claims: CLAIMS,
            notes: NOTES,
        };
        let descriptor = &DESCRIPTOR;
        assert_eq!(descriptor.crate_name, "nmp-test");
        assert_eq!(descriptor.claims[0].scope_value, "7");
        assert!(descriptor.claims[0].exclusive);
        assert_eq!(descriptor.notes[0].claim, "nostr.kind.7.reaction");
    }

    #[test]
    fn protected_reaction_requires_nip25_artifact_provenance() {
        let proof = EventOwnershipProvenance::new(
            Some(ArtifactProvenance::new(
                "nmp.nip25",
                "nostr.kind.7.reaction",
            )),
            &[],
        );
        assert!(validate_publish_ownership(7, &[], Some(proof), false).is_ok());
        assert!(validate_publish_ownership(7, &[], None, false).is_err());
    }

    #[test]
    fn kind5_deletion_requires_nip09_artifact_provenance() {
        // A kind:5 draft minted by nmp-nip09 must carry nmp.nip09 provenance.
        let nip09_proof = EventOwnershipProvenance::new(
            Some(ArtifactProvenance::new(
                "nmp.nip09",
                "nostr.kind.5.deletion",
            )),
            &[],
        );
        // Passes with nip09 provenance regardless of tags (generic gate).
        assert!(validate_publish_ownership(5, &[], Some(nip09_proof), false).is_ok());
        // Fails without any provenance.
        assert!(validate_publish_ownership(5, &[], None, false).is_err());
        // Fails with wrong provenance (e.g. old nip25 claim — no longer accepted).
        let old_nip25_proof = EventOwnershipProvenance::new(
            Some(ArtifactProvenance::new(
                "nmp.nip25",
                "nostr.kind.5.delete_kind_7_reaction",
            )),
            &[],
        );
        assert!(validate_publish_ownership(5, &[], Some(old_nip25_proof), false).is_err());
    }

    #[test]
    fn nip09_minted_reaction_deletion_passes_gate() {
        // Reaction retraction: nip25 delegates to nip09 → provenance is nip09.
        // This test documents the composed publish path (ADR-0074).
        let nip09_proof = EventOwnershipProvenance::new(
            Some(ArtifactProvenance::new(
                "nmp.nip09",
                "nostr.kind.5.deletion",
            )),
            &[],
        );
        let tags = vec![vec!["e".to_string(), "a".repeat(64)]];
        assert!(validate_publish_ownership(5, &tags, Some(nip09_proof), false).is_ok());
    }

    #[test]
    fn h_envelope_requires_nip29_provenance_and_group_pin() {
        const ENVELOPES: &[EnvelopeProvenance] = &[EnvelopeProvenance::new(
            "nmp.nip29",
            "nostr.nip29.group_envelope",
        )];
        let proof = EventOwnershipProvenance::new(None, ENVELOPES);
        let tags = vec![vec!["h".to_string(), "room".to_string()]];
        assert!(validate_publish_ownership(9, &tags, Some(proof), true).is_ok());
        assert!(validate_publish_ownership(9, &tags, Some(proof), false).is_err());
        assert!(validate_publish_ownership(9, &tags, None, true).is_err());
    }

    #[test]
    fn owned_draft_carries_event_and_provenance_together() {
        let proof = EventOwnershipProvenance::new(
            Some(ArtifactProvenance::new("nmp.test", "test.claim")),
            &[],
        );
        let draft = OwnedEventDraft::new("event", proof);
        assert_eq!(draft.event(), &"event");
        assert_eq!(draft.ownership(), proof);
        let mapped = draft.map_event(|event| format!("{event}-mapped"));
        let (event, ownership) = mapped.into_parts();
        assert_eq!(event, "event-mapped");
        assert_eq!(ownership, proof);
    }
}

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;

//! Positive ownership descriptor for `nmp-nip09`.
//!
//! `nmp-nip09` holds **exclusive** artifact ownership of the generic NIP-09
//! deletion wire claim (`nostr.kind.5.deletion`). Other crates that initiate
//! deletion (e.g. `nmp-nip25` reaction retraction) hold **non-exclusive** intent
//! claims under a scoped context predicate; they do NOT own the kind:5 wire
//! grammar. See ADR-0074 for the full composable-ownership doctrine.

nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip09",
    crate_name: "nmp-nip09",
    summary: "NIP-09 generic deletion (kind:5) artifact ownership for NMP apps.",
    claims: [
        {
            claim_type: "artifact",
            id: "nostr.kind.5.deletion",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "5",
                context: "",
            },
            owns: [
                "kind:5 deletion construction",
                "deleted-event e tag grammar",
                "deleted-kind k tag grammar",
                "deleted-address a tag grammar (address-coordinate targets)",
                "canonical address-coordinate identity (kind:pubkey:d) shared by every a-tag reader/writer",
                "deletion content/reason rules",
                "deletion identity rules",
                "generic deletion read semantics (DeleteRecord)",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.nip09.delete",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.nip09.delete",
                context: "",
            },
            owns: [
                "typed generic deletion action namespace",
            ],
        },
    ],
    notes: [],
}

/// Artifact provenance token for a kind:5 deletion built by `nmp-nip09`.
pub const DELETION_ARTIFACT: nmp_ownership::ArtifactProvenance =
    nmp_ownership::ArtifactProvenance::new("nmp.nip09", "nostr.kind.5.deletion");

/// Full event-ownership provenance for a kind:5 deletion draft minted by
/// `nmp-nip09`. Carried by every [`crate::OwnedDeletionDraft`] produced by
/// this crate's builders.
pub const DELETION_EVENT_PROVENANCE: nmp_ownership::EventOwnershipProvenance =
    nmp_ownership::EventOwnershipProvenance::new(Some(DELETION_ARTIFACT), &[]);

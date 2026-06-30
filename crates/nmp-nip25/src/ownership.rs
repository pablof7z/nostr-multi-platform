nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip25",
    crate_name: "nmp-nip25",
    summary: "NIP-25 public reaction actions and bounded viewer reaction projection for NMP apps.",
    claims: [
        {
            claim_type: "artifact",
            id: "nostr.kind.7.reaction",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "7",
                context: "",
            },
            owns: [
                "reaction construction",
                "reaction target tag grammar",
                "reaction content normalization",
                "viewer reaction identity",
                "reaction aggregate projection",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.kind.5.delete_kind_7_reaction",
            exclusive: false,
            scope: {
                kind: "kind",
                value: "5",
                context: "deletes-kind-7-reaction",
            },
            owns: [
                "reaction retraction intent",
                "deleted reaction id validation",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.nip25.react",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.nip25.react",
                context: "",
            },
            owns: [
                "typed reaction action namespace",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.nip25.unreact",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.nip25.unreact",
                context: "",
            },
            owns: [
                "typed reaction retraction action namespace",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.nmp.nip25.reactions",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "nmp.nip25.reactions",
                context: "",
            },
            owns: [
                "reaction aggregate projection key",
            ],
        },
    ],
    notes: [
    ],
}

pub const REACTION_ARTIFACT: nmp_ownership::ArtifactProvenance =
    nmp_ownership::ArtifactProvenance::new("nmp.nip25", "nostr.kind.7.reaction");

pub const REACTION_DELETE_ARTIFACT: nmp_ownership::ArtifactProvenance =
    nmp_ownership::ArtifactProvenance::new("nmp.nip25", "nostr.kind.5.delete_kind_7_reaction");

pub const REACTION_EVENT_PROVENANCE: nmp_ownership::EventOwnershipProvenance =
    nmp_ownership::EventOwnershipProvenance::new(Some(REACTION_ARTIFACT), &[]);

pub const REACTION_DELETE_EVENT_PROVENANCE: nmp_ownership::EventOwnershipProvenance =
    nmp_ownership::EventOwnershipProvenance::new(Some(REACTION_DELETE_ARTIFACT), &[]);

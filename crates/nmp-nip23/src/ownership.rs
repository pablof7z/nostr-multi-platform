nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip23",
    crate_name: "nmp-nip23",
    summary: "NIP-23 long-form article protocol projections, feed semantics, and typed sidecar ownership.",
    claims: [
        {
            claim_type: "artifact",
            id: "nostr.kind.30023.long_form_article",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "30023",
                context: "",
            },
            owns: [
                "NIP-23 long-form article projection semantics",
                "addressable-coordinate article supersession",
                "long-form feed admission and delete folding",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.nmp.nip23.articles",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "nmp.nip23.articles",
                context: "",
            },
            owns: [
                "NIP-23 long-form article typed projection key",
                "NL23 long-form article FlatBuffers sidecar schema",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.kind.5.delete_kind_30023_article",
            exclusive: false,
            scope: {
                kind: "kind",
                value: "5",
                context: "deletes-kind-30023-article",
            },
            owns: [
                "NIP-09 delete folding for long-form article projections",
            ],
        },
    ],
    notes: [
        {
            claim: "nostr.kind.30023.long_form_article",
            text: "Article body tokenization and rendering remain delegated to nmp-content; this crate owns the NIP-23 protocol read model.",
        },
    ],
}

pub const LONG_FORM_ARTICLE_ARTIFACT: nmp_ownership::ArtifactProvenance =
    nmp_ownership::ArtifactProvenance::new("nmp.nip23", "nostr.kind.30023.long_form_article");

pub const LONG_FORM_ARTICLE_EVENT_PROVENANCE: nmp_ownership::EventOwnershipProvenance =
    nmp_ownership::EventOwnershipProvenance::new(Some(LONG_FORM_ARTICLE_ARTIFACT), &[]);

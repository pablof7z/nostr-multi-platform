nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip01",
    crate_name: "nmp-nip01",
    summary: "NIP-01 short text notes (kind:1) relation surface as an NMP protocol crate - NoteRecord decoder (+ NIP-10 refs), NoteBuilder, RepliesView + ThreadView per docs/design/kind-wrappers.md section 3 and view-catalog section 5. Relation read-views + note builder only; kernel timeline extraction is a separate effort.",
    claims: [
        {
            claim_type: "artifact",
            id: "nostr.kind.1.short_text_note",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "1",
                context: "",
            },
            owns: [
                "short text note construction",
                "note relation parsing",
                "timeline/thread note read semantics",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.kind.0.profile_metadata",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "0",
                context: "",
            },
            owns: [
                "profile metadata parsing and read semantics",
            ],
        },
    ],
    notes: [
    ],
}

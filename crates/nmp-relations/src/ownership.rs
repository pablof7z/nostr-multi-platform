nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.relations",
    crate_name: "nmp-relations",
    summary: "Cross-protocol visible-note relation classifier and visible-row relation interest action. It owns the action namespace only, not event kinds or engagement semantics.",
    claims: [
        {
            claim_type: "namespace",
            id: "action.nmp.nip01.visible_note_relations",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.nip01.visible_note_relations",
                context: "",
            },
            owns: [
                "visible note relation interest action namespace",
            ],
        },
    ],
    notes: [
    ],
}

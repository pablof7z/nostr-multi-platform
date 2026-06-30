nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip02",
    crate_name: "nmp-nip02",
    summary: "NIP-02 follow-list actions and projections for NMP apps.",
    claims: [
        {
            claim_type: "artifact",
            id: "nostr.kind.3.contact_list",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "3",
                context: "",
            },
            owns: [
                "contact/follow list actions and projections",
            ],
        },
    ],
    notes: [
    ],
}

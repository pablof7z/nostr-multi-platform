nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.substrate",
    crate_name: "nmp-substrate",
    summary: "Reusable substrate construction floor: shared mailbox/profile/contacts cache-parser wiring, routing, publish resolver, coverage, blocked-relay, NIP-77, and native NIP-11 hooks.",
    claims: [
        {
            claim_type: "mechanism",
            id: "substrate.install_floor",
            exclusive: true,
            scope: {
                kind: "function",
                value: "nmp_substrate::install",
                context: "",
            },
            owns: [
                "shared NMP substrate installation floor",
            ],
        },
    ],
    notes: [
    ],
}

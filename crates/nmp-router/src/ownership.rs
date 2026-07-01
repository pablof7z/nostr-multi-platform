nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.router",
    crate_name: "nmp-router",
    summary: "Layer-2 routing: one generic OutboxRouter algorithm (NIP-65 mailbox + relay hints + p-tag inbox + indexer eligibility), the NIP-65-only InMemoryMailboxCache, kind:10002 IngestParser/cache writer, and indexer-republish policy. Step 2 of docs/architecture/crate-boundaries.md.",
    claims: [
        {
            claim_type: "mechanism",
            id: "router.compiled_plan_execution",
            exclusive: true,
            scope: {
                kind: "type",
                value: "Router",
                context: "",
            },
            owns: [
                "execution of planner-produced relay plans",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.nip65.publish_relay_list",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.nip65.publish_relay_list",
                context: "",
            },
            owns: [
                "NIP-65 relay-list publish action namespace",
            ],
        },
    ],
    notes: [
    ],
}

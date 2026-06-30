nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.sqlite_wasm",
    crate_name: "nmp-sqlite-wasm",
    summary: "OPFS-backed SQLite EventStore backend for wasm32 (#1007). Spine only - the SQLite engine + EventStore impl land in later PRs.",
    claims: [
        {
            claim_type: "mechanism",
            id: "store.sqlite_wasm_backend",
            exclusive: true,
            scope: {
                kind: "backend",
                value: "sqlite-wasm",
                context: "",
            },
            owns: [
                "SQLite/WASM event-store backend implementation",
            ],
        },
    ],
    notes: [
    ],
}

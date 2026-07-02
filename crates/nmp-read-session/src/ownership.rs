nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.read_session",
    crate_name: "nmp-read-session",
    summary: "Concept-neutral read-lifecycle engine: one open/replace/close registry, replay-before-live ordering, exact-demand withdrawal, reverse teardown, and typed-output tombstone behind every concept-owned active read.",
    claims: [
        {
            claim_type: "mechanism",
            id: "read_session.lifecycle_engine",
            exclusive: true,
            scope: {
                kind: "type",
                value: "ReadSessionRegistry",
                context: "",
            },
            owns: [
                "the single read-lifecycle implementation: handle allocation, open/replace/close registry, replay-before-live ordering, live activation, exact-demand withdrawal, reverse teardown, typed-output tombstone, and one leak audit for all concept-owned active reads",
            ],
        },
    ],
    notes: [
        {
            claim: "read_session.lifecycle_engine",
            text: "Concept crates supply a declarative ReadSpec (demand, admission-applying reducer, typed output) and drive it through open_read/close_read; they must not re-author replay, live activation, registry replacement, exact close, reverse teardown, or tombstone emission (#2777).",
        },
    ],
}

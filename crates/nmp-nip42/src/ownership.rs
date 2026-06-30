nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip42",
    crate_name: "nmp-nip42",
    summary: "NIP-42 relay AUTH as an NMP protocol crate. Owns the kind:22242 builder and the per-relay handshake driver; the shared wire/type vocabulary (RelayAuthState, AUTH/OK frame shapes + parsers) lives in nmp-nip42-types; the wire-frame pause/flush is owned by nmp-core::subs::AuthGate.",
    claims: [
        {
            claim_type: "artifact",
            id: "nostr.kind.22242.nip42_auth",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "22242",
                context: "",
            },
            owns: [
                "NIP-42 AUTH event builder and relay handshake driver",
            ],
        },
    ],
    notes: [
    ],
}

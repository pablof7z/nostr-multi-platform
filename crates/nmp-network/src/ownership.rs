nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.network",
    crate_name: "nmp-network",
    summary: "Layer-1 network transport: native WebSocket relay worker (tungstenite/mio/rustls) + wasm32 browser relay driver (web_sys::WebSocket), wire-transport-agnostic relay protocol primitives (backoff, keepalive, jitter, HTTP-denial classifier). See docs/architecture/crate-boundaries.md section 8 - extraction, push-model Pool, BrowserRelayDriver, and NIP-46 actor-Pool lane (nmp-nip46-runtime) all shipped.",
    claims: [
        {
            claim_type: "mechanism",
            id: "network.relay_socket",
            exclusive: true,
            scope: {
                kind: "type",
                value: "RelayConnection",
                context: "",
            },
            owns: [
                "relay socket transport substrate",
            ],
        },
    ],
    notes: [
    ],
}

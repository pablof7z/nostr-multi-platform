nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.browser_runtime",
    crate_name: "nmp-browser-runtime",
    summary: "Browser platform adapter for NMP: Worker event loop, browser relay transport, capability/signer provider registry, and the BrowserAppBuilder app-composition target. This crate (superseding the retired nmp-wasm) owns the browser runtime.",
    claims: [
        {
            claim_type: "mechanism",
            id: "browser_runtime.worker_bridge",
            exclusive: true,
            scope: {
                kind: "type",
                value: "BrowserRuntime",
                context: "",
            },
            owns: [
                "browser worker runtime and host bridge semantics",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.nmp.notifications",
            exclusive: true,
            scope: {
                kind: "projection_family",
                value: "nmp.notifications*",
                context: "",
            },
            owns: [
                "browser notification typed projection family",
            ],
        },
    ],
    notes: [
    ],
}

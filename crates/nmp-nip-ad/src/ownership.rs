nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip_ad",
    crate_name: "nmp-nip-ad",
    summary: "NIP-AD web-URL → Nostr-events resolver (#2927). Resolves an ordinary `https://<domain>/<path>` URL by fetching `.well-known/nostr.json?ad=<path>`, selecting the entry keyed by the requested path, and yielding a live `{filter, relays}` collection query (0..N events). Ships the app-injected `AdResolutionPolicy` seam. Shape/parse is pure; the HTTP round-trip is a blocking worker (native feature).",
    claims: [
        {
            claim_type: "mechanism",
            id: "nip_ad.url_resolution",
            exclusive: true,
            scope: {
                kind: "resolver",
                value: "nip-ad",
                context: "",
            },
            owns: [
                "NIP-AD URL parsing, `.well-known` ad-query resolution, and auto-resolution policy semantics",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.nmp.nip-ad.collection",
            exclusive: true,
            scope: {
                kind: "projection_family",
                value: "nmp.nip-ad.collection.*",
                context: "",
            },
            owns: [
                "per-session NIP-AD collection result projection family (the `open_ad_collection` delivery doorway, #2948)",
            ],
        },
    ],
    notes: [
    ],
}

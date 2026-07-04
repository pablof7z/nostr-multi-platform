nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.wellknown_http",
    crate_name: "nmp-wellknown-http",
    summary: "Shared SSRF-guarded bounded HTTP GET for `.well-known` fetches (#2927). Single workspace home for `assert_host_is_public` (reject IP-literal / non-public-resolving hosts) and `http_get_json(url, max_bytes)` (bounded GET, 10s timeout, redirects(0)). Consumed by nmp-nip05 and nmp-nip-ad (host-guarded) and nmp-nip57 (bounded GET). Native-gated.",
    claims: [
        {
            claim_type: "mechanism",
            id: "wellknown_http.ssrf_guarded_get",
            exclusive: true,
            scope: {
                kind: "mechanism",
                value: "wellknown-http",
                context: "",
            },
            owns: [
                "SSRF host-vetting and bounded `.well-known` HTTP GET semantics",
            ],
        },
    ],
    notes: [
    ],
}

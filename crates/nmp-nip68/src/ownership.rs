nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip68",
    crate_name: "nmp-nip68",
    summary: "NIP-68 picture-first feed primitives for NMP: kind:20 PictureEventRecord decoder, NIP-92 imeta parser, and PicturePostBuilder blueprint.",
    claims: [
        {
            claim_type: "artifact",
            id: "nostr.kind.20.picture_event",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "20",
                context: "",
            },
            owns: [
                "NIP-68 picture event decode, feed projection, and builder blueprint",
            ],
        },
    ],
    notes: [
    ],
}

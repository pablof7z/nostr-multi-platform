nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip51",
    crate_name: "nmp-nip51",
    summary: "NIP-51 list projections for NMP: mute suppression, global bookmarks, bookmark/curation sets, web bookmarks, and search-relay facts. Protocol parsing stays here; generic routing/search policy stays in substrate/search owners.",
    claims: [
        {
            claim_type: "artifact",
            id: "nostr.nip51.lists",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "10000",
                context: "",
            },
            owns: [
                "NIP-51 list parsing, actions, and projections",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.nip51.lists",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "10003",
                context: "",
            },
            owns: [
                "NIP-51 list parsing, actions, and projections",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.nip51.lists",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "10006",
                context: "",
            },
            owns: [
                "NIP-51 list parsing, actions, and projections",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.nip51.lists",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "10007",
                context: "",
            },
            owns: [
                "NIP-51 list parsing, actions, and projections",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.nip51.lists",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "30000",
                context: "",
            },
            owns: [
                "NIP-51 list parsing, actions, and projections",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.nip51.lists",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "30003",
                context: "",
            },
            owns: [
                "NIP-51 list parsing, actions, and projections",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.nip51.lists",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "30004",
                context: "",
            },
            owns: [
                "NIP-51 list parsing, actions, and projections",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.nip51.lists",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "39701",
                context: "",
            },
            owns: [
                "NIP-51 list parsing, actions, and projections",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.nmp.nip51.mute_list",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "nmp.nip51.mute_list",
                context: "",
            },
            owns: [
                "mute-list projection key",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.nmp.nip51.bookmarks",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "nmp.nip51.bookmarks",
                context: "",
            },
            owns: [
                "bookmarks projection key",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.nip51.add_bookmark",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.nip51.add_bookmark",
                context: "",
            },
            owns: [
                "bookmark action namespace",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.nip51.remove_bookmark",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.nip51.remove_bookmark",
                context: "",
            },
            owns: [
                "bookmark action namespace",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.nip51.add_bookmark_set_item",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.nip51.add_bookmark_set_item",
                context: "",
            },
            owns: [
                "bookmark-set action namespace",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.nip51.remove_bookmark_set_item",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.nip51.remove_bookmark_set_item",
                context: "",
            },
            owns: [
                "bookmark-set action namespace",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.nip51.publish_web_bookmark",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.nip51.publish_web_bookmark",
                context: "",
            },
            owns: [
                "web bookmark action namespace",
            ],
        },
    ],
    notes: [
    ],
}

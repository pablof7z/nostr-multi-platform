nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip29",
    crate_name: "nmp-nip29",
    summary: "NIP-29 relay-based groups: group artifacts, group envelopes, previous-chain context, and host-relay routing inputs. It owns group context and routing inputs, not wrapped artifact semantics.",
    claims: [
        {
            claim_type: "artifact",
            id: "nostr.nip29.group_metadata",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "39000",
                context: "",
            },
            owns: [
                "relay-signed group metadata snapshots",
                "group admin/member/role snapshot semantics",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.nip29.group_metadata",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "39001",
                context: "",
            },
            owns: [
                "relay-signed group metadata snapshots",
                "group admin/member/role snapshot semantics",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.nip29.group_metadata",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "39002",
                context: "",
            },
            owns: [
                "relay-signed group metadata snapshots",
                "group admin/member/role snapshot semantics",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.nip29.group_metadata",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "39003",
                context: "",
            },
            owns: [
                "relay-signed group metadata snapshots",
                "group admin/member/role snapshot semantics",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.nip29.group_management",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "9000",
                context: "",
            },
            owns: [
                "group moderation/admin/member/invite action semantics",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.nip29.group_management",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "9001",
                context: "",
            },
            owns: [
                "group moderation/admin/member/invite action semantics",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.nip29.group_management",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "9002",
                context: "",
            },
            owns: [
                "group moderation/admin/member/invite action semantics",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.nip29.group_management",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "9005",
                context: "",
            },
            owns: [
                "group moderation/admin/member/invite action semantics",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.nip29.group_management",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "9007",
                context: "",
            },
            owns: [
                "group moderation/admin/member/invite action semantics",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.nip29.group_management",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "9008",
                context: "",
            },
            owns: [
                "group moderation/admin/member/invite action semantics",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.nip29.group_management",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "9009",
                context: "",
            },
            owns: [
                "group moderation/admin/member/invite action semantics",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.nip29.group_management",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "9021",
                context: "",
            },
            owns: [
                "group moderation/admin/member/invite action semantics",
            ],
        },
        {
            claim_type: "artifact",
            id: "nostr.nip29.group_management",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "9022",
                context: "",
            },
            owns: [
                "group moderation/admin/member/invite action semantics",
            ],
        },
        {
            claim_type: "envelope",
            id: "nostr.nip29.group_envelope",
            exclusive: true,
            scope: {
                kind: "tag",
                value: "h",
                context: "nip29.group_context",
            },
            owns: [
                "group context envelope",
                "host relay pin requirement",
            ],
        },
        {
            claim_type: "envelope",
            id: "nostr.nip29.previous_chain",
            exclusive: true,
            scope: {
                kind: "tag",
                value: "previous",
                context: "nip29.group_publish",
            },
            owns: [
                "group previous-chain injection",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.nip29.publish_group_event",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.nip29.publish_group_event",
                context: "",
            },
            owns: [
                "group envelope publish action namespace",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.nmp.nip29.group_events",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "nmp.nip29.group_events",
                context: "",
            },
            owns: [
                "group event projection key",
            ],
        },
    ],
    notes: [
        {
            claim: "nostr.nip29.group_envelope",
            text: "The h tag scopes and routes a wrapped artifact; it does not transfer artifact-kind ownership to nmp-nip29.",
        },
    ],
}

pub const GROUP_METADATA_ARTIFACT: nmp_ownership::ArtifactProvenance =
    nmp_ownership::ArtifactProvenance::new("nmp.nip29", "nostr.nip29.group_metadata");

pub const GROUP_MANAGEMENT_ARTIFACT: nmp_ownership::ArtifactProvenance =
    nmp_ownership::ArtifactProvenance::new("nmp.nip29", "nostr.nip29.group_management");

pub const GROUP_ENVELOPE: nmp_ownership::EnvelopeProvenance =
    nmp_ownership::EnvelopeProvenance::new("nmp.nip29", "nostr.nip29.group_envelope");

pub const PREVIOUS_CHAIN: nmp_ownership::EnvelopeProvenance =
    nmp_ownership::EnvelopeProvenance::new("nmp.nip29", "nostr.nip29.previous_chain");

pub const GROUP_ENVELOPE_ONLY: &[nmp_ownership::EnvelopeProvenance] = &[GROUP_ENVELOPE];

pub const GROUP_ENVELOPE_WITH_PREVIOUS: &[nmp_ownership::EnvelopeProvenance] =
    &[GROUP_ENVELOPE, PREVIOUS_CHAIN];

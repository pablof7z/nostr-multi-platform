nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.core",
    crate_name: "nmp-core",
    summary: "NMP kernel substrate: actor runtime, state, capabilities, action registry, snapshots, and protocol extension seams.",
    claims: [
        {
            claim_type: "mechanism",
            id: "core.actor_runtime",
            exclusive: true,
            scope: {
                kind: "type",
                value: "AppHost",
                context: "",
            },
            owns: [
                "kernel actor lifecycle",
                "single-writer action dispatch",
            ],
        },
        {
            claim_type: "mechanism",
            id: "core.publish_pipeline",
            exclusive: true,
            scope: {
                kind: "type",
                value: "PublishCommand",
                context: "",
            },
            owns: [
                "signing and publish command pipeline",
            ],
        },
        {
            claim_type: "mechanism",
            id: "core.projection_substrate",
            exclusive: true,
            scope: {
                kind: "type",
                value: "ObservedProjectionSink",
                context: "",
            },
            owns: [
                "projection registration and snapshot emission",
            ],
        },
        {
            claim_type: "namespace",
            id: "core.action_registry",
            exclusive: true,
            scope: {
                kind: "registry",
                value: "ActionRegistry",
                context: "",
            },
            owns: [
                "typed action module registry",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.profile",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "profile",
                context: "",
            },
            owns: [
                "profile projection key",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.accounts",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "accounts",
                context: "",
            },
            owns: [
                "accounts projection key",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.active_account",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "active_account",
                context: "",
            },
            owns: [
                "active_account projection key",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.configured_relays",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "configured_relays",
                context: "",
            },
            owns: [
                "configured_relays projection key",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.relay_role_options",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "relay_role_options",
                context: "",
            },
            owns: [
                "relay_role_options projection key",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.settings_hub",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "settings_hub",
                context: "",
            },
            owns: [
                "settings_hub projection key",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.publish_queue",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "publish_queue",
                context: "",
            },
            owns: [
                "publish_queue projection key",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.publish_outbox",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "publish_outbox",
                context: "",
            },
            owns: [
                "publish_outbox projection key",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.outbox_summary",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "outbox_summary",
                context: "",
            },
            owns: [
                "outbox_summary projection key",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.action_results",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "action_results",
                context: "",
            },
            owns: [
                "action_results projection key",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.signed_events",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "signed_events",
                context: "",
            },
            owns: [
                "signed_events projection key",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.action_stages",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "action_stages",
                context: "",
            },
            owns: [
                "action_stages projection key",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.action_lifecycle",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "action_lifecycle",
                context: "",
            },
            owns: [
                "action_lifecycle projection key",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.relay_diagnostics",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "relay_diagnostics",
                context: "",
            },
            owns: [
                "relay_diagnostics projection key",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.refs.profile",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "refs.profile",
                context: "",
            },
            owns: [
                "refs.profile projection key",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.refs.event",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "refs.event",
                context: "",
            },
            owns: [
                "refs.event projection key",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.bunker_handshake",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "bunker_handshake",
                context: "",
            },
            owns: [
                "bunker_handshake projection key",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.nip46_onboarding",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "nip46_onboarding",
                context: "",
            },
            owns: [
                "nip46_onboarding projection key",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.signer_state",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "signer_state",
                context: "",
            },
            owns: [
                "signer_state projection key",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.publish",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.publish",
                context: "",
            },
            owns: [
                "publish action namespace",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.browse_relay",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.browse_relay",
                context: "",
            },
            owns: [
                "relay browse action namespace",
            ],
        },
    ],
    notes: [
    ],
}

//! ADR-0064 §3 (#1783) — the `ACTION_BUILDERS` flat-table data, split out of
//! `registry.rs` to stay under the 500-LOC ceiling (mirrors the
//! `action_contract.rs` + `action_contract/table.rs` split). The neutral
//! schema/file-id facts live in `crate::action_contract::ACTION_CONTRACT`; the
//! field ORDER here is load-bearing and MUST match each `.fbs` table.

use super::{ActionBuilder, FieldKind, PayloadField};

/// The flat-table builders (ADR-0064 §3 acceptance scope).
///
/// `nmp.publish` carries a FlatBuffers UNION body rather than a flat table, so
/// it does NOT live here — its encode shape (a nested body table + a union type
/// discriminant) is materially different from the flat-table primitives below.
/// The publish builders are described by [`PUBLISH_BUILDERS`] and hand-modelled
/// by the emitters' `render_publish_*` paths. This registry covers every
/// flat-table namespace end-to-end: every primitive (string, uint, optional
/// string, string vector, ulong-with-presence-flag, relay-list-entry-vec) is
/// exercised.
pub const ACTION_BUILDERS: &[ActionBuilder] = &[
    // nip25 — react / unreact (react.fbs / unreact.fbs).
    ActionBuilder {
        namespace: "nmp.nip25.react",
        method: "react",
        fields: &[
            PayloadField {
                name: "targetEventId",
                kind: FieldKind::Str,
                optional: false,
            },
            PayloadField {
                name: "reaction",
                kind: FieldKind::Str,
                optional: false,
            },
            PayloadField {
                name: "targetAuthorPubkey",
                kind: FieldKind::Str,
                optional: true,
            },
        ],
        doc: "Publish a NIP-25 reaction to a target event.",
    },
    ActionBuilder {
        namespace: "nmp.nip25.unreact",
        method: "unreact",
        fields: &[
            PayloadField {
                name: "reactionEventId",
                kind: FieldKind::Str,
                optional: false,
            },
            PayloadField {
                name: "reason",
                kind: FieldKind::Str,
                optional: false,
            },
        ],
        doc: "Retract a previously-published NIP-25 reaction.",
    },
    // nip01 — publish a kind:1 short-text note. Rust owns ALL NIP-10 reply-tag
    // construction (publish_note.fbs); the host passes raw content + the parent's
    // protocol fields when replying. Field order MUST match the `.fbs` slots.
    ActionBuilder {
        namespace: "nmp.nip01.publish_note",
        method: "publishNote",
        fields: &[
            PayloadField {
                name: "content",
                kind: FieldKind::Str,
                optional: false,
            },
            PayloadField {
                name: "replyEventId",
                kind: FieldKind::Str,
                optional: true,
            },
            PayloadField {
                name: "replyAuthorPubkey",
                kind: FieldKind::Str,
                optional: true,
            },
            PayloadField {
                name: "replyRootEventId",
                kind: FieldKind::Str,
                optional: true,
            },
            PayloadField {
                name: "replyRootRelay",
                kind: FieldKind::Str,
                optional: true,
            },
            PayloadField {
                name: "replyMentionedPubkeys",
                kind: FieldKind::StrVec,
                optional: true,
            },
        ],
        doc: "Publish a NIP-01 kind:1 note; Rust builds NIP-10 reply tags from the parent fields.",
    },
    // nip18 — repost a kind:1 note as a kind:6 wrapper. Rust builds the
    // `["e", event_id]` + `["p", author_pubkey]` tags (repost.fbs).
    ActionBuilder {
        namespace: "nmp.nip18.repost",
        method: "repost",
        fields: &[
            PayloadField {
                name: "eventId",
                kind: FieldKind::Str,
                optional: false,
            },
            PayloadField {
                name: "authorPubkey",
                kind: FieldKind::Str,
                optional: false,
            },
        ],
        doc: "Repost an event (NIP-18 kind:6); Rust builds the e/p tags.",
    },
    // nip02 — follow / unfollow share the single-pubkey FollowActionPayload
    // shape (follow_action.fbs); follow_many is the bulk primitive
    // (follow_many_action.fbs).
    ActionBuilder {
        namespace: "nmp.follow",
        method: "follow",
        fields: &[PayloadField {
            name: "pubkey",
            kind: FieldKind::Str,
            optional: false,
        }],
        doc: "Follow a single pubkey (NIP-02 contact-list add).",
    },
    ActionBuilder {
        namespace: "nmp.unfollow",
        method: "unfollow",
        fields: &[PayloadField {
            name: "pubkey",
            kind: FieldKind::Str,
            optional: false,
        }],
        doc: "Unfollow a single pubkey (NIP-02 contact-list remove).",
    },
    ActionBuilder {
        namespace: "nmp.follow_many",
        method: "followMany",
        fields: &[PayloadField {
            name: "pubkeys",
            kind: FieldKind::StrVec,
            optional: true,
        }],
        doc: "Follow many pubkeys in one race-free read-modify-write cycle (NIP-02).",
    },
    // nip51 — block / unblock relay (block_relay.fbs / unblock_relay.fbs).
    ActionBuilder {
        namespace: "nmp.nip51.block_relay",
        method: "blockRelay",
        fields: &[
            PayloadField {
                name: "url",
                kind: FieldKind::Str,
                optional: false,
            },
            PayloadField {
                name: "accountPubkey",
                kind: FieldKind::Str,
                optional: false,
            },
        ],
        doc: "Add a relay URL to the NIP-51 blocked-relay list.",
    },
    ActionBuilder {
        namespace: "nmp.nip51.unblock_relay",
        method: "unblockRelay",
        fields: &[
            PayloadField {
                name: "url",
                kind: FieldKind::Str,
                optional: false,
            },
            PayloadField {
                name: "accountPubkey",
                kind: FieldKind::Str,
                optional: false,
            },
        ],
        doc: "Remove a relay URL from the NIP-51 blocked-relay list.",
    },
    // nip17 — DM send (send.fbs). Rust owns the kind:14 rumor + gift-wrap.
    ActionBuilder {
        namespace: "nmp.nip17.send",
        method: "sendDm",
        fields: &[
            PayloadField {
                name: "recipientPubkey",
                kind: FieldKind::Str,
                optional: false,
            },
            PayloadField {
                name: "content",
                kind: FieldKind::Str,
                optional: false,
            },
            PayloadField {
                name: "replyTo",
                kind: FieldKind::Str,
                optional: true,
            },
        ],
        doc: "Send a NIP-17 private direct message (kind:14 → gift-wrapped kind:1059).",
    },
    // nip17 — DM relay list (dm_relay_list_action.fbs).
    ActionBuilder {
        namespace: "nmp.nip17.publish_relay_list",
        method: "publishDmRelayList",
        fields: &[PayloadField {
            name: "relays",
            kind: FieldKind::StrVec,
            optional: false,
        }],
        doc: "Publish a NIP-17 DM relay list (kind:10050).",
    },
    // nip57 — lightning zap (zap.fbs). Rust owns the kind:9734 build + LNURL
    // round-trip. Field order MUST match the `.fbs` slots.
    ActionBuilder {
        namespace: "nmp.nip57.zap",
        method: "zap",
        fields: &[
            PayloadField {
                name: "recipientPubkey",
                kind: FieldKind::Str,
                optional: false,
            },
            PayloadField {
                name: "amountMsats",
                kind: FieldKind::Ulong,
                optional: false,
            },
            PayloadField {
                name: "lnurl",
                kind: FieldKind::Str,
                optional: true,
            },
            PayloadField {
                name: "relays",
                kind: FieldKind::StrVec,
                optional: false,
            },
            PayloadField {
                name: "targetEventId",
                kind: FieldKind::Str,
                optional: true,
            },
            PayloadField {
                name: "comment",
                kind: FieldKind::Str,
                optional: true,
            },
        ],
        doc: "Zap a recipient (NIP-57 kind:9734); Rust owns LNURL resolution + relay selection.",
    },
    // nip65 — outbox relay list (publish_relay_list.fbs).
    ActionBuilder {
        namespace: "nmp.nip65.publish_relay_list",
        method: "publishRelayList",
        fields: &[PayloadField {
            name: "relays",
            kind: FieldKind::RelayListEntryVec,
            optional: false,
        }],
        doc: "Publish a NIP-65 relay-list metadata event (kind:10002).",
    },
    // wallet — NIP-47 Nostr Wallet Connect (wallet_connect.fbs /
    // wallet_disconnect.fbs / wallet_pay_invoice.fbs).
    ActionBuilder {
        namespace: "nmp.wallet.connect",
        method: "walletConnect",
        fields: &[PayloadField {
            name: "uri",
            kind: FieldKind::Str,
            optional: false,
        }],
        doc: "Connect a NIP-47 Nostr Wallet Connect URI.",
    },
    ActionBuilder {
        namespace: "nmp.wallet.disconnect",
        method: "walletDisconnect",
        fields: &[],
        doc: "Disconnect the current NIP-47 wallet (no payload data beyond schema_version).",
    },
    ActionBuilder {
        namespace: "nmp.wallet.pay_invoice",
        method: "walletPayInvoice",
        fields: &[
            PayloadField {
                name: "bolt11",
                kind: FieldKind::Str,
                optional: false,
            },
            // `amount_msats` is `Option<u64>` on the Rust side — two slots:
            // the ulong scalar + a `has_amount_msats:bool` presence flag.
            PayloadField {
                name: "amountMsats",
                kind: FieldKind::UlongWithPresenceFlag {
                    flag_name: "hasAmountMsats",
                },
                optional: true,
            },
        ],
        doc: "Pay a Lightning invoice via the NIP-47 wallet.",
    },
];

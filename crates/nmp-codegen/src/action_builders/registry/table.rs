//! ADR-0064 §3 (#1783) — the flat-table action-builder data table.
//!
//! Split out of [`super`] (`registry.rs`) as a size-management seam: the parent
//! module owns the types + the union/publish/marmot builders; this submodule
//! owns ONLY the flat `ACTION_BUILDERS` slice so the parent stays under the
//! file-size gate. The native action-boundary gate
//! (`ci/check_native_action_boundary.py`) scans `registry/*.rs`, so the moved
//! namespace literals stay visible to the gate.

use super::host_called::{
    ADD_BOOKMARK_SET_ITEM, BLOSSOM_UPLOAD, BROWSE_RELAY, CREATE_PUBLIC_GROUP, DISCOVER_GROUPS,
    JOIN_GROUP, POST_COMMENT, PUBLISH_GROUP_EVENT, PUBLISH_HIGHLIGHT, PUBLISH_WEB_BOOKMARK,
    REMOVE_BOOKMARK_SET_ITEM, TOPIC_ARTICLES,
};
use super::{ActionBuilder, FieldKind, PayloadField};

/// The flat-table builders (ADR-0064 §3 acceptance scope).
///
/// `nmp.publish` carries a FlatBuffers UNION body rather than a flat table, so
/// it does NOT live here — its encode shape (a nested body table + a union type
/// discriminant) is materially different from the flat-table primitives below.
/// The publish builders are described by [`PUBLISH_BUILDERS`] and hand-modelled
/// by the emitters' `render_publish_*` paths. This registry covers every
/// flat-table namespace end-to-end: every primitive (string, uint, optional
/// string, string vector, uint vector, ulong-with-presence-flag,
/// relay-list-entry-vec) is exercised.
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
    // nip18 — publish repost wrappers from target facts. Rust owns tag shape.
    ActionBuilder {
        namespace: "nmp.nip18.repost",
        method: "repost",
        fields: &[
            PayloadField {
                name: "targetEventId",
                kind: FieldKind::Str,
                optional: false,
            },
            PayloadField {
                name: "targetKind",
                kind: FieldKind::Uint,
                optional: false,
            },
            PayloadField {
                name: "targetAuthorPubkey",
                kind: FieldKind::Str,
                optional: true,
            },
            PayloadField {
                name: "relayHint",
                kind: FieldKind::Str,
                optional: true,
            },
        ],
        doc: "Publish a NIP-18 repost wrapper for a target event.",
    },
    ActionBuilder {
        namespace: "nmp.nip18.quote_repost",
        method: "quoteRepost",
        fields: &[
            PayloadField {
                name: "targetEventId",
                kind: FieldKind::Str,
                optional: false,
            },
            PayloadField {
                name: "targetKind",
                kind: FieldKind::Uint,
                optional: false,
            },
            PayloadField {
                name: "targetAuthorPubkey",
                kind: FieldKind::Str,
                optional: true,
            },
            PayloadField {
                name: "relayHint",
                kind: FieldKind::Str,
                optional: true,
            },
            PayloadField {
                name: "content",
                kind: FieldKind::Str,
                optional: false,
            },
        ],
        doc: "Publish a NIP-18 quote repost note for a target event.",
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
    // nip51 — bookmark add/remove share BookmarkUpdatePayload. This is a nested
    // item table, so the emitters special-case the method shape while still
    // keeping the namespace/method/doc in this generated-builder registry.
    ActionBuilder {
        namespace: "nmp.nip51.add_bookmark",
        method: "addBookmark",
        fields: &[],
        doc: "Add one item to the active account's NIP-51 bookmark list.",
    },
    ActionBuilder {
        namespace: "nmp.nip51.remove_bookmark",
        method: "removeBookmark",
        fields: &[],
        doc: "Remove one item from the active account's NIP-51 bookmark list.",
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
    ActionBuilder {
        namespace: "nmp.nip17.hydrate_peer_relay_list",
        method: "hydrateDmPeerRelayList",
        fields: &[PayloadField {
            name: "peerPubkey",
            kind: FieldKind::Str,
            optional: false,
        }],
        doc: "Hydrate a DM peer's NIP-17 relay list (kind:10050).",
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
    // nip17 — send a NIP-17 gift-wrapped DM (send.fbs / SendDmPayload). Mirrors
    // `nmp_nip17::SendDmInput`: recipient_pubkey + content required, reply_to
    // optional (absent → None on decode).
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
        doc: "Send a NIP-17 gift-wrapped direct message to a recipient.",
    },
    // nip57 — publish a NIP-57 zap request (zap.fbs / ZapPayload). Mirrors
    // `nmp_nip57::ZapInput`: recipient_pubkey required + amount_msats (u64)
    // inline; lnurl / target_event_id / comment optional; relays is the vector
    // (may be empty → kernel auto-selects from NIP-65 write relays).
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
        doc: "Publish a NIP-57 zap request for a recipient (optionally a target event).",
    },
    // Host-called typed actions added after the original registry slice.
    PUBLISH_HIGHLIGHT,
    POST_COMMENT,
    ADD_BOOKMARK_SET_ITEM,
    REMOVE_BOOKMARK_SET_ITEM,
    PUBLISH_WEB_BOOKMARK,
    BLOSSOM_UPLOAD,
    BROWSE_RELAY,
    TOPIC_ARTICLES,
    // NIP-29 group actions (issue #2170).
    DISCOVER_GROUPS,
    PUBLISH_GROUP_EVENT,
    JOIN_GROUP,
    CREATE_PUBLIC_GROUP,
];

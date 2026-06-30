//! Host-called typed action builders added after the original flat-table
//! registry. Kept separate so `table.rs` remains an aggregate list under the
//! hand-authored file-size gate.

use super::{ActionBuilder, FieldKind, PayloadField};

pub(super) const PUBLISH_HIGHLIGHT: ActionBuilder = ActionBuilder {
    namespace: "nmp.nip84.publish_highlight",
    method: "publishHighlight",
    fields: &[
        PayloadField {
            name: "content",
            kind: FieldKind::Str,
            optional: false,
        },
        PayloadField {
            name: "context",
            kind: FieldKind::Str,
            optional: true,
        },
        PayloadField {
            name: "sourceEventId",
            kind: FieldKind::Str,
            optional: true,
        },
        PayloadField {
            name: "sourceAddress",
            kind: FieldKind::Str,
            optional: true,
        },
        PayloadField {
            name: "sourceAuthorPubkey",
            kind: FieldKind::Str,
            optional: true,
        },
        PayloadField {
            name: "alt",
            kind: FieldKind::Str,
            optional: true,
        },
        PayloadField {
            name: "externalIds",
            kind: FieldKind::StrVec,
            optional: true,
        },
    ],
    doc: "Publish a NIP-84 kind:9802 highlight annotation.",
};

pub(super) const REPLY: ActionBuilder = ActionBuilder {
    namespace: "nmp.replies.reply",
    method: "reply",
    fields: &[
        PayloadField {
            name: "targetEventId",
            kind: FieldKind::Str,
            optional: true,
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
            name: "targetAddress",
            kind: FieldKind::Str,
            optional: true,
        },
        PayloadField {
            name: "targetExternalUri",
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
    doc: "Publish a reply; Rust chooses NIP-10 kind:1 or NIP-22 kind:1111 from the target.",
};

pub(super) const ADD_BOOKMARK_SET_ITEM: ActionBuilder = ActionBuilder {
    namespace: "nmp.nip51.add_bookmark_set_item",
    method: "addBookmarkSetItem",
    fields: &[],
    doc: "Add an item to a NIP-51 kind:30003 bookmark or kind:30004 curation set.",
};

pub(super) const REMOVE_BOOKMARK_SET_ITEM: ActionBuilder = ActionBuilder {
    namespace: "nmp.nip51.remove_bookmark_set_item",
    method: "removeBookmarkSetItem",
    fields: &[],
    doc: "Remove an item from a NIP-51 kind:30003 bookmark or kind:30004 curation set.",
};

pub(super) const PUBLISH_WEB_BOOKMARK: ActionBuilder = ActionBuilder {
    namespace: "nmp.nip51.publish_web_bookmark",
    method: "publishWebBookmark",
    fields: &[
        PayloadField {
            name: "accountPubkey",
            kind: FieldKind::Str,
            optional: false,
        },
        PayloadField {
            name: "url",
            kind: FieldKind::Str,
            optional: false,
        },
        PayloadField {
            name: "title",
            kind: FieldKind::Str,
            optional: true,
        },
        PayloadField {
            name: "description",
            kind: FieldKind::Str,
            optional: true,
        },
        PayloadField {
            name: "publishedAt",
            kind: FieldKind::UlongWithPresenceFlag {
                flag_name: "hasPublishedAt",
            },
            optional: true,
        },
        PayloadField {
            name: "hashtags",
            kind: FieldKind::StrVec,
            optional: true,
        },
    ],
    doc: "Publish or update a NIP-B0 kind:39701 web bookmark.",
};

pub(super) const BLOSSOM_UPLOAD: ActionBuilder = ActionBuilder {
    namespace: "nmp.blossom.upload",
    method: "blossomUpload",
    fields: &[
        PayloadField {
            name: "filePath",
            kind: FieldKind::Str,
            optional: false,
        },
        PayloadField {
            name: "contentType",
            kind: FieldKind::Str,
            optional: true,
        },
        PayloadField {
            name: "servers",
            kind: FieldKind::StrVec,
            optional: true,
        },
        PayloadField {
            name: "signerPubkey",
            kind: FieldKind::Str,
            optional: true,
        },
    ],
    doc: "Upload a file via BUD-02 to one or more Blossom servers.",
};

pub(super) const BROWSE_RELAY: ActionBuilder = ActionBuilder {
    namespace: "nmp.browse_relay",
    method: "browseRelay",
    fields: &[
        PayloadField {
            name: "op",
            kind: FieldKind::Ubyte,
            optional: false,
        },
        PayloadField {
            name: "relayUrl",
            kind: FieldKind::Str,
            optional: true,
        },
        PayloadField {
            name: "kinds",
            kind: FieldKind::UintVec,
            optional: true,
        },
        PayloadField {
            name: "lifecycle",
            kind: FieldKind::Ubyte,
            optional: false,
        },
        PayloadField {
            name: "interestId",
            kind: FieldKind::Ulong,
            optional: false,
        },
    ],
    doc: "Open or close a relay-pinned browse subscription.",
};

pub(super) const VISIBLE_NOTE_RELATIONS: ActionBuilder = ActionBuilder {
    namespace: "nmp.nip01.visible_note_relations",
    method: "visibleNoteRelations",
    fields: &[
        PayloadField {
            name: "lifecycle",
            kind: FieldKind::Ubyte,
            optional: false,
        },
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
            name: "consumerId",
            kind: FieldKind::Str,
            optional: false,
        },
        PayloadField {
            name: "targetAddress",
            kind: FieldKind::Str,
            optional: true,
        },
    ],
    doc: "Claim or release visible note relation-count subscriptions.",
};

// ── NIP-29 group actions ──────────────────────────────────────────────────────

pub(super) const DISCOVER_GROUPS: ActionBuilder = ActionBuilder {
    namespace: "nmp.nip29.discover",
    method: "discoverGroups",
    fields: &[PayloadField {
        name: "relayUrl",
        kind: FieldKind::Str,
        optional: false,
    }],
    doc: "Discover NIP-29 groups hosted on a relay.",
};

pub(super) const PUBLISH_GROUP_EVENT: ActionBuilder = ActionBuilder {
    namespace: "nmp.nip29.publish_group_event",
    method: "publishGroupEvent",
    fields: &[
        PayloadField {
            name: "group",
            kind: FieldKind::GroupRef,
            optional: false,
        },
        PayloadField {
            name: "kind",
            kind: FieldKind::Uint,
            optional: false,
        },
        PayloadField {
            name: "content",
            kind: FieldKind::Str,
            optional: true,
        },
        PayloadField {
            name: "tags",
            kind: FieldKind::StringTagVec,
            optional: true,
        },
    ],
    doc: "Publish an event to a NIP-29 group (any kind).",
};

pub(super) const JOIN_GROUP: ActionBuilder = ActionBuilder {
    namespace: "nmp.nip29.join",
    method: "joinGroup",
    fields: &[
        PayloadField {
            name: "group",
            kind: FieldKind::GroupRef,
            optional: false,
        },
        PayloadField {
            name: "inviteCode",
            kind: FieldKind::Str,
            optional: true,
        },
        PayloadField {
            name: "reason",
            kind: FieldKind::Str,
            optional: true,
        },
    ],
    doc: "Request membership in a NIP-29 group.",
};

pub(super) const EDIT_METADATA: ActionBuilder = ActionBuilder {
    namespace: "nmp.nip29.edit_metadata",
    method: "editGroupMetadata",
    fields: &[
        PayloadField {
            name: "group",
            kind: FieldKind::GroupRef,
            optional: false,
        },
        PayloadField {
            name: "name",
            kind: FieldKind::Str,
            optional: true,
        },
        PayloadField {
            name: "about",
            kind: FieldKind::Str,
            optional: true,
        },
        PayloadField {
            name: "picture",
            kind: FieldKind::Str,
            optional: true,
        },
        // Tri-state byte enums (0 = Unset → leave prior value). The host passes
        // the raw discriminant; omit (optional) → Unset.
        PayloadField {
            name: "visibility",
            kind: FieldKind::Sbyte,
            optional: true,
        },
        PayloadField {
            name: "access",
            kind: FieldKind::Sbyte,
            optional: true,
        },
    ],
    doc: "Edit an existing NIP-29 group's name/about/picture/visibility/access.",
};

pub(super) const CREATE_PUBLIC_GROUP: ActionBuilder = ActionBuilder {
    namespace: "nmp.nip29.create_public_group",
    method: "createPublicGroup",
    fields: &[
        PayloadField {
            name: "group",
            kind: FieldKind::GroupRef,
            optional: false,
        },
        PayloadField {
            name: "name",
            kind: FieldKind::Str,
            optional: false,
        },
        PayloadField {
            name: "about",
            kind: FieldKind::Str,
            optional: true,
        },
        PayloadField {
            name: "picture",
            kind: FieldKind::Str,
            optional: true,
        },
        PayloadField {
            name: "visibility",
            kind: FieldKind::Sbyte,
            optional: false,
        },
        PayloadField {
            name: "access",
            kind: FieldKind::Sbyte,
            optional: false,
        },
        PayloadField {
            name: "parent",
            kind: FieldKind::Str,
            optional: true,
        },
    ],
    doc: "Create a new public NIP-29 group.",
};

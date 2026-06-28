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

pub(super) const POST_COMMENT: ActionBuilder = ActionBuilder {
    namespace: "nmp.nip22.post_comment",
    method: "postComment",
    fields: &[
        PayloadField {
            name: "rootTagName",
            kind: FieldKind::Str,
            optional: false,
        },
        PayloadField {
            name: "rootTagValue",
            kind: FieldKind::Str,
            optional: false,
        },
        PayloadField {
            name: "rootKind",
            kind: FieldKind::Uint,
            optional: false,
        },
        PayloadField {
            name: "parentEventId",
            kind: FieldKind::Str,
            optional: true,
        },
        PayloadField {
            name: "rootAuthorPubkey",
            kind: FieldKind::Str,
            optional: true,
        },
        PayloadField {
            name: "parentAuthorPubkey",
            kind: FieldKind::Str,
            optional: true,
        },
        PayloadField {
            name: "content",
            kind: FieldKind::Str,
            optional: false,
        },
    ],
    doc: "Publish a NIP-22 kind:1111 comment.",
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

pub(super) const VISIBLE_NOTE_RELATIONS: ActionBuilder = ActionBuilder {
    namespace: "nmp.nip01.visible_note_relations",
    method: "visibleNoteRelations",
    fields: &[
        PayloadField {
            name: "op",
            kind: FieldKind::Ubyte,
            optional: false,
        },
        PayloadField {
            name: "eventId",
            kind: FieldKind::Str,
            optional: false,
        },
        PayloadField {
            name: "consumerId",
            kind: FieldKind::Str,
            optional: false,
        },
    ],
    doc: "Claim or release the tailing interest for a note's visible relations (NIP-01).",
};

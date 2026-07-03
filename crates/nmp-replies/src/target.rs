use nmp_core::substrate::KernelEvent;
use nmp_kinds::{KIND_NIP22_COMMENT, KIND_SHORT_TEXT_NOTE};
use nmp_nip01::{try_from_kernel_event as note_from_kernel_event, Nip10Refs, NoteRecord};
use nmp_nip22::{try_from_kernel_event as comment_from_kernel_event, CommentRecord};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ReplyTarget {
    Note(NoteRecord),
    Comment(CommentRecord),
    Event(ReplyEventTarget),
    Address(ReplyAddressTarget),
    External(ReplyExternalTarget),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReplyEventTarget {
    pub event_id: String,
    pub kind: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_pubkey: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReplyAddressTarget {
    pub coordinate: String,
    pub kind: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_pubkey: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReplyExternalTarget {
    pub uri: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ReplyTargetError {
    EmptyTarget,
    InvalidEventId,
    InvalidAuthorPubkey,
    MissingTargetAuthor,
    CommentEventRequiresRecord,
}

impl core::fmt::Display for ReplyTargetError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyTarget => write!(f, "reply target must not be empty"),
            Self::InvalidEventId => write!(f, "reply event target must be a 64-hex event id"),
            Self::InvalidAuthorPubkey => write!(f, "reply target author must be 64-hex"),
            Self::MissingTargetAuthor => {
                write!(f, "kind:1 reply target requires the parent author pubkey")
            }
            Self::CommentEventRequiresRecord => {
                write!(f, "kind:1111 reply target requires a decoded CommentRecord")
            }
        }
    }
}

impl std::error::Error for ReplyTargetError {}

impl ReplyTargetError {
    /// The stable machine code (crosses the wire as an FFI error code; never
    /// renumbered or repurposed — #2899 Part A).
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::EmptyTarget => "empty_target",
            Self::InvalidEventId => "invalid_event_id",
            Self::InvalidAuthorPubkey => "invalid_author_pubkey",
            Self::MissingTargetAuthor => "missing_target_author",
            Self::CommentEventRequiresRecord => "comment_event_requires_record",
        }
    }
}

impl ReplyTarget {
    #[must_use]
    pub fn note(record: NoteRecord) -> Self {
        Self::Note(record)
    }

    #[must_use]
    pub fn comment(record: CommentRecord) -> Self {
        Self::Comment(record)
    }

    pub fn event(
        event_id: impl Into<String>,
        kind: u32,
        author_pubkey: Option<String>,
    ) -> Result<Self, ReplyTargetError> {
        let event_id = event_id.into().trim().to_string();
        if !is_hex64(&event_id) {
            return Err(ReplyTargetError::InvalidEventId);
        }
        if author_pubkey
            .as_deref()
            .map(str::trim)
            .is_some_and(|author| !author.is_empty() && !is_hex64(author))
        {
            return Err(ReplyTargetError::InvalidAuthorPubkey);
        }
        Ok(Self::Event(ReplyEventTarget {
            event_id,
            kind,
            author_pubkey: trimmed_optional(author_pubkey),
        }))
    }

    pub fn address(
        coordinate: impl Into<String>,
        kind: u32,
        author_pubkey: Option<String>,
    ) -> Result<Self, ReplyTargetError> {
        let coordinate = coordinate.into().trim().to_string();
        if coordinate.is_empty() {
            return Err(ReplyTargetError::EmptyTarget);
        }
        if author_pubkey
            .as_deref()
            .map(str::trim)
            .is_some_and(|author| !author.is_empty() && !is_hex64(author))
        {
            return Err(ReplyTargetError::InvalidAuthorPubkey);
        }
        Ok(Self::Address(ReplyAddressTarget {
            coordinate,
            kind,
            author_pubkey: trimmed_optional(author_pubkey),
        }))
    }

    pub fn external(uri: impl Into<String>) -> Result<Self, ReplyTargetError> {
        let uri = uri.into().trim().to_string();
        if uri.is_empty() {
            return Err(ReplyTargetError::EmptyTarget);
        }
        Ok(Self::External(ReplyExternalTarget { uri }))
    }

    #[must_use]
    pub fn from_kernel_event(event: &KernelEvent) -> Self {
        if let Some(note) = note_from_kernel_event(event) {
            return Self::Note(note);
        }
        if let Some(comment) = comment_from_kernel_event(event) {
            return Self::Comment(comment);
        }
        Self::Event(ReplyEventTarget {
            event_id: event.id.clone(),
            kind: event.kind,
            author_pubkey: Some(event.author.clone()),
        })
    }

    pub(crate) fn note_record_for_event(
        target: &ReplyEventTarget,
    ) -> Result<NoteRecord, ReplyTargetError> {
        if target.kind == KIND_NIP22_COMMENT {
            return Err(ReplyTargetError::CommentEventRequiresRecord);
        }
        let Some(author) = target.author_pubkey.clone() else {
            return Err(ReplyTargetError::MissingTargetAuthor);
        };
        if !is_hex64(&target.event_id) {
            return Err(ReplyTargetError::InvalidEventId);
        }
        if !is_hex64(&author) {
            return Err(ReplyTargetError::InvalidAuthorPubkey);
        }
        Ok(NoteRecord {
            event_id: target.event_id.clone(),
            author,
            created_at: 0,
            content: String::new(),
            refs: Nip10Refs::default(),
        })
    }

    /// The raw identifier this target's reply summary is keyed by: the event id
    /// (note/comment/event), the addressable coordinate, or the external URI.
    #[must_use]
    pub fn summary_token(&self) -> String {
        match self {
            Self::Note(note) => note.event_id.clone(),
            Self::Comment(comment) => comment.event_id.clone(),
            Self::Event(event) => event.event_id.clone(),
            Self::Address(address) => address.coordinate.clone(),
            Self::External(external) => external.uri.clone(),
        }
    }

    pub(crate) fn is_nip10(&self) -> bool {
        matches!(self, Self::Note(_))
            || matches!(self, Self::Event(event) if event.kind == KIND_SHORT_TEXT_NOTE)
    }
}

pub(crate) fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

pub(crate) fn trimmed_optional(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// A flat, FFI-marshalable [`ReplyTarget`] input for [`crate::open_replies`]
/// (#2899 Part A bridge lane), mirroring `nmp-native-runtime`'s
/// `decode_and_validate_feed_params` decode+validate posture — reactions/
/// reposts/zaps already take a plain `target_event_id: String`, so only
/// `nmp-replies`' multi-shape target needs this.
///
/// # Why `Event` cannot cover a kind:1111 comment target too
///
/// A kind:1111 NIP-22 comment target needs its OWN tag-decoded root/parent
/// scope (`CommentRecord`) to compute the correct query (`nmp-replies`'
/// internal `nip22_anchor` reads `comment.root_tag_name` / `root_tag_value`
/// for it). That is NIP-22 tag grammar owned by `nmp-nip22`
/// (`try_from_kernel_event`); re-deriving it from a bare
/// `event_id`/`kind`/`author_pubkey` scalar would mean re-implementing that
/// grammar at the FFI boundary (a D0 violation), and there is no way to
/// invent the root scope from those three fields anyway. So [`Self::Event`]
/// **rejects `kind: 1111`** up front with the existing
/// [`ReplyTargetError::CommentEventRequiresRecord`] code, and callers with a
/// kind:1111 target use [`Self::Comment`], which carries the target's raw
/// kernel-event fields and decodes them through the real NIP-22 decoder —
/// exactly what [`ReplyTarget::from_kernel_event`] already does internally.
/// A plain kind:1 note target still goes through [`Self::Event`] (unaffected:
/// only its `event_id` is read downstream — see [`ReplyTarget::is_nip10`]).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "target_type", rename_all = "snake_case")]
pub enum ReplyTargetParams {
    /// Mirrors [`ReplyTarget::event`]. Rejected for `kind: 1111` — see the
    /// type-level docs; use [`Self::Comment`] instead.
    Event {
        event_id: String,
        kind: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        author_pubkey: Option<String>,
    },
    /// A kind:1111 NIP-22 comment target, decoded from its own raw
    /// kernel-event fields via `nmp_nip22::try_from_kernel_event` — the only
    /// correct way to supply one (see the type-level docs).
    Comment {
        event_id: String,
        author_pubkey: String,
        created_at: u64,
        /// Raw NIP-22 tag array (e.g. `[["E", "…"], ["K", "1"], ["e", "…"],
        /// ["k", "1"]]`) — untouched, decoded by `nmp-nip22`.
        tags: Vec<Vec<String>>,
        content: String,
    },
    /// Mirrors [`ReplyTarget::address`] — an addressable (NIP-33) target;
    /// there is no single underlying kernel event to decode.
    Address {
        coordinate: String,
        kind: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        author_pubkey: Option<String>,
    },
    /// Mirrors [`ReplyTarget::external`] — a non-Nostr (NIP-73) external
    /// target.
    External { uri: String },
}

/// Typed error for FFI-boundary [`ReplyTargetParams`] decode + validation
/// (D6 — fail-closed, never panics).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReplyTargetParamsError {
    /// The JSON payload did not parse into a [`ReplyTargetParams`].
    MalformedJson,
    /// The decoded params failed [`ReplyTarget`] construction/validation.
    InvalidTarget(ReplyTargetError),
}

impl core::fmt::Display for ReplyTargetParamsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MalformedJson => write!(f, "reply target params must be valid JSON"),
            Self::InvalidTarget(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for ReplyTargetParamsError {}

impl ReplyTargetParamsError {
    /// The stable machine code (crosses the wire as an FFI error code).
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::MalformedJson => "malformed_json",
            Self::InvalidTarget(err) => err.code(),
        }
    }
}

/// Decode + validate a [`ReplyTargetParams`] JSON payload into a
/// [`ReplyTarget`], the validated FFI-marshalable target input for
/// [`crate::open_replies`] (#2899 Part A).
///
/// # Errors
///
/// * [`ReplyTargetParamsError::MalformedJson`] — `json` is not valid JSON, or
///   does not match a [`ReplyTargetParams`] variant.
/// * [`ReplyTargetParamsError::InvalidTarget`] — decoded but the target
///   construction rejected it (e.g. a non-hex event id, or a kind:1111
///   [`ReplyTargetParams::Event`] that must use [`ReplyTargetParams::Comment`]
///   instead).
pub fn decode_and_validate_reply_target(
    json: &str,
) -> Result<ReplyTarget, ReplyTargetParamsError> {
    let params: ReplyTargetParams =
        serde_json::from_str(json).map_err(|_| ReplyTargetParamsError::MalformedJson)?;
    match params {
        ReplyTargetParams::Event {
            event_id,
            kind,
            author_pubkey,
        } => {
            if kind == KIND_NIP22_COMMENT {
                return Err(ReplyTargetParamsError::InvalidTarget(
                    ReplyTargetError::CommentEventRequiresRecord,
                ));
            }
            ReplyTarget::event(event_id, kind, author_pubkey)
                .map_err(ReplyTargetParamsError::InvalidTarget)
        }
        ReplyTargetParams::Comment {
            event_id,
            author_pubkey,
            created_at,
            tags,
            content,
        } => {
            if !is_hex64(event_id.trim()) {
                return Err(ReplyTargetParamsError::InvalidTarget(
                    ReplyTargetError::InvalidEventId,
                ));
            }
            if !is_hex64(author_pubkey.trim()) {
                return Err(ReplyTargetParamsError::InvalidTarget(
                    ReplyTargetError::InvalidAuthorPubkey,
                ));
            }
            let event = KernelEvent {
                id: event_id,
                author: author_pubkey,
                kind: KIND_NIP22_COMMENT,
                created_at,
                tags,
                content,
                relay_provenance: Vec::new(),
            };
            comment_from_kernel_event(&event)
                .map(ReplyTarget::Comment)
                .ok_or(ReplyTargetParamsError::InvalidTarget(
                    ReplyTargetError::CommentEventRequiresRecord,
                ))
        }
        ReplyTargetParams::Address {
            coordinate,
            kind,
            author_pubkey,
        } => ReplyTarget::address(coordinate, kind, author_pubkey)
            .map_err(ReplyTargetParamsError::InvalidTarget),
        ReplyTargetParams::External { uri } => {
            ReplyTarget::external(uri).map_err(ReplyTargetParamsError::InvalidTarget)
        }
    }
}

#[cfg(test)]
#[path = "target_tests.rs"]
mod tests;

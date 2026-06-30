use nmp_core::substrate::KernelEvent;
use nmp_core::tags::Nip10Refs;
use nmp_kinds::{KIND_NIP22_COMMENT, KIND_SHORT_TEXT_NOTE};
use nmp_nip01::{try_from_kernel_event as note_from_kernel_event, NoteRecord};
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

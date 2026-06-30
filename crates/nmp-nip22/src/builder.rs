//! Pure NIP-22 comment event builder.
//!
//! This module owns the raw kind:1111 tag shape. It is intentionally not an
//! app action: `nmp-replies` owns the app-facing decision of whether a reply is
//! NIP-10/kind:1 or NIP-22/kind:1111.

use nmp_kinds::KIND_NIP22_COMMENT;
use nmp_signer_iface::UnsignedEvent;
use serde::{Deserialize, Serialize};

use crate::decode::CommentRecord;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CommentRoot {
    Event {
        event_id: String,
        kind: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        author_pubkey: Option<String>,
    },
    Address {
        coordinate: String,
        kind: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        author_pubkey: Option<String>,
    },
    External {
        uri: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CommentParent {
    Root,
    Comment {
        event_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        author_pubkey: Option<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CommentBuildInput {
    pub root: CommentRoot,
    pub parent: CommentParent,
    pub content: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CommentBuildError {
    EmptyContent,
    EmptyRoot,
    InvalidEventId { field: String },
    InvalidAuthorPubkey { field: String },
    InvalidRootKind { value: String },
    InvalidRootScope { value: String },
}

impl core::fmt::Display for CommentBuildError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyContent => write!(f, "NIP-22 comment content must not be empty"),
            Self::EmptyRoot => write!(f, "NIP-22 comment root must not be empty"),
            Self::InvalidEventId { field } => {
                write!(f, "NIP-22 comment {field} must be a 64-hex event id")
            }
            Self::InvalidAuthorPubkey { field } => {
                write!(f, "NIP-22 comment {field} must be a 64-hex pubkey")
            }
            Self::InvalidRootKind { value } => {
                write!(f, "NIP-22 comment root kind must be a u32, got {value:?}")
            }
            Self::InvalidRootScope { value } => {
                write!(f, "NIP-22 comment root scope must be A/E/I, got {value:?}")
            }
        }
    }
}

impl std::error::Error for CommentBuildError {}

impl CommentBuildInput {
    #[must_use]
    pub fn top_level(root: CommentRoot, content: impl Into<String>) -> Self {
        Self {
            root,
            parent: CommentParent::Root,
            content: content.into(),
        }
    }

    /// Build a child comment under an already decoded kind:1111 comment.
    ///
    /// The caller supplies a decoded record, not root-scope strings. This keeps
    /// root/parent reconstruction in Rust protocol code.
    pub fn reply_to_comment(
        parent: &CommentRecord,
        content: impl Into<String>,
    ) -> Result<Self, CommentBuildError> {
        Ok(Self {
            root: root_from_comment(parent)?,
            parent: CommentParent::Comment {
                event_id: parent.event_id.clone(),
                author_pubkey: non_empty(parent.author_pubkey.as_str()),
            },
            content: content.into(),
        })
    }
}

pub fn build_comment_event(
    input: CommentBuildInput,
    author: impl Into<String>,
    created_at: u64,
) -> Result<UnsignedEvent, CommentBuildError> {
    let content = input.content.trim().to_string();
    if content.is_empty() {
        return Err(CommentBuildError::EmptyContent);
    }
    let author = author.into();
    if !author.is_empty() && !is_hex64(&author) {
        return Err(CommentBuildError::InvalidAuthorPubkey {
            field: "author".to_string(),
        });
    }

    let RootParts {
        upper,
        lower,
        value,
        kind,
        author_pubkey,
    } = root_parts(input.root)?;
    let mut tags = Vec::with_capacity(6);
    tags.push(vec![upper.to_string(), value.clone()]);
    tags.push(vec!["K".to_string(), kind.to_string()]);
    if let Some(root_author) = author_pubkey {
        tags.push(vec!["P".to_string(), root_author]);
    }

    match input.parent {
        CommentParent::Root => {
            tags.push(vec![lower.to_string(), value]);
            tags.push(vec!["k".to_string(), kind.to_string()]);
        }
        CommentParent::Comment {
            event_id,
            author_pubkey,
        } => {
            let parent = event_id.trim().to_string();
            if !is_hex64(&parent) {
                return Err(CommentBuildError::InvalidEventId {
                    field: "parent comment event_id".to_string(),
                });
            }
            tags.push(vec!["e".to_string(), parent]);
            tags.push(vec!["k".to_string(), KIND_NIP22_COMMENT.to_string()]);
            if let Some(parent_author) = checked_pubkey(author_pubkey, "parent author_pubkey")? {
                tags.push(vec!["p".to_string(), parent_author]);
            }
        }
    }

    Ok(UnsignedEvent {
        pubkey: author,
        kind: KIND_NIP22_COMMENT,
        tags,
        content,
        created_at,
    })
}

struct RootParts {
    upper: char,
    lower: char,
    value: String,
    kind: u32,
    author_pubkey: Option<String>,
}

fn root_parts(root: CommentRoot) -> Result<RootParts, CommentBuildError> {
    match root {
        CommentRoot::Event {
            event_id,
            kind,
            author_pubkey,
        } => {
            let value = event_id.trim().to_string();
            if !is_hex64(&value) {
                return Err(CommentBuildError::InvalidEventId {
                    field: "root event_id".to_string(),
                });
            }
            Ok(RootParts {
                upper: 'E',
                lower: 'e',
                value,
                kind,
                author_pubkey: checked_pubkey(author_pubkey, "root author_pubkey")?,
            })
        }
        CommentRoot::Address {
            coordinate,
            kind,
            author_pubkey,
        } => {
            let value = coordinate.trim().to_string();
            if value.is_empty() {
                return Err(CommentBuildError::EmptyRoot);
            }
            Ok(RootParts {
                upper: 'A',
                lower: 'a',
                value,
                kind,
                author_pubkey: checked_pubkey(author_pubkey, "root author_pubkey")?,
            })
        }
        CommentRoot::External { uri } => {
            let value = uri.trim().to_string();
            if value.is_empty() {
                return Err(CommentBuildError::EmptyRoot);
            }
            Ok(RootParts {
                upper: 'I',
                lower: 'i',
                value,
                kind: 0,
                author_pubkey: None,
            })
        }
    }
}

fn root_from_comment(record: &CommentRecord) -> Result<CommentRoot, CommentBuildError> {
    let kind =
        if record.root_kind.trim().is_empty() {
            0
        } else {
            record.root_kind.trim().parse::<u32>().map_err(|_| {
                CommentBuildError::InvalidRootKind {
                    value: record.root_kind.clone(),
                }
            })?
        };
    match record.root_tag_name.as_str() {
        "E" => Ok(CommentRoot::Event {
            event_id: record.root_tag_value.clone(),
            kind,
            author_pubkey: non_empty(&record.root_author_pubkey),
        }),
        "A" => Ok(CommentRoot::Address {
            coordinate: record.root_tag_value.clone(),
            kind,
            author_pubkey: non_empty(&record.root_author_pubkey),
        }),
        "I" => Ok(CommentRoot::External {
            uri: record.root_tag_value.clone(),
        }),
        other => Err(CommentBuildError::InvalidRootScope {
            value: other.to_string(),
        }),
    }
}

fn checked_pubkey(value: Option<String>, field: &str) -> Result<Option<String>, CommentBuildError> {
    let Some(value) = value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    else {
        return Ok(None);
    };
    if !is_hex64(&value) {
        return Err(CommentBuildError::InvalidAuthorPubkey {
            field: field.to_string(),
        });
    }
    Ok(Some(value))
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
#[path = "builder_tests.rs"]
mod tests;

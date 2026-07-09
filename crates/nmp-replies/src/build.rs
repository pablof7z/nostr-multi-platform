use nmp_kinds::{KIND_NIP22_COMMENT, KIND_SHORT_TEXT_NOTE};
use nmp_nip01::{Note, NoteBuildError};
use nmp_nip22::{
    build_comment_event, CommentBuildError, CommentBuildInput, CommentParent, CommentRoot,
};
use nmp_signer_iface::UnsignedEvent;
use serde::{Deserialize, Serialize};

use crate::target::{ReplyTarget, ReplyTargetError};

pub struct Reply;

#[derive(Clone, Debug)]
pub struct ReplyBuilder {
    target: ReplyTarget,
    content: String,
    relay_hint: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ReplyBuildError {
    Target(ReplyTargetError),
    Note(NoteBuildError),
    Comment(CommentBuildError),
}

impl core::fmt::Display for ReplyBuildError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Target(err) => write!(f, "{err}"),
            Self::Note(err) => write!(f, "{err}"),
            Self::Comment(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for ReplyBuildError {}

impl From<ReplyTargetError> for ReplyBuildError {
    fn from(value: ReplyTargetError) -> Self {
        Self::Target(value)
    }
}

impl From<NoteBuildError> for ReplyBuildError {
    fn from(value: NoteBuildError) -> Self {
        Self::Note(value)
    }
}

impl From<CommentBuildError> for ReplyBuildError {
    fn from(value: CommentBuildError) -> Self {
        Self::Comment(value)
    }
}

impl Reply {
    #[must_use]
    pub fn to(target: ReplyTarget, content: impl Into<String>) -> ReplyBuilder {
        ReplyBuilder {
            target,
            content: content.into(),
            relay_hint: None,
        }
    }
}

impl ReplyBuilder {
    #[must_use]
    pub fn relay_hint(mut self, relay: impl Into<String>) -> Self {
        let relay = relay.into().trim().to_string();
        self.relay_hint = if relay.is_empty() { None } else { Some(relay) };
        self
    }

    pub fn build(
        self,
        author: impl Into<String>,
        created_at: u64,
    ) -> Result<UnsignedEvent, ReplyBuildError> {
        match self.target {
            ReplyTarget::Note(parent) => {
                let mut builder = Note::new(self.content);
                if let Some(relay) = self.relay_hint {
                    builder = builder.relay_hint(relay);
                }
                Ok(builder.reply_to(&parent).build(author, created_at)?)
            }
            ReplyTarget::Event(event) if event.kind == KIND_SHORT_TEXT_NOTE => {
                // #3099: a kind:1 `Event` target only ever reaches here when the
                // parent was NOT read from the local cache (a cache hit returns
                // `ReplyTarget::Note` with the parent's real NIP-10 refs — see
                // `crate::action::resolve_event_target`). Never fabricate a
                // root/reply shape from that cache miss; fail closed.
                Err(ReplyTarget::reject_uncached_note_parent(&event).into())
            }
            ReplyTarget::Comment(parent) => {
                let input = CommentBuildInput::reply_to_comment(&parent, self.content)?;
                Ok(build_comment_event(input, author, created_at)?)
            }
            ReplyTarget::Event(event) if event.kind == KIND_NIP22_COMMENT => {
                Err(ReplyTargetError::CommentEventRequiresRecord.into())
            }
            ReplyTarget::Event(event) => Ok(build_comment_event(
                CommentBuildInput::top_level(
                    CommentRoot::Event {
                        event_id: event.event_id,
                        kind: event.kind,
                        author_pubkey: event.author_pubkey,
                    },
                    self.content,
                ),
                author,
                created_at,
            )?),
            ReplyTarget::Address(address) => Ok(build_comment_event(
                CommentBuildInput::top_level(
                    CommentRoot::Address {
                        coordinate: address.coordinate,
                        kind: address.kind,
                        author_pubkey: address.author_pubkey,
                    },
                    self.content,
                ),
                author,
                created_at,
            )?),
            ReplyTarget::External(external) => Ok(build_comment_event(
                CommentBuildInput {
                    root: CommentRoot::External { uri: external.uri },
                    parent: CommentParent::Root,
                    content: self.content,
                },
                author,
                created_at,
            )?),
        }
    }
}

#[cfg(test)]
#[path = "build_tests.rs"]
mod tests;

//! Per-category scenario builders. Each returns `ScenarioDto`s whose
//! `rendered` field is produced by the **real** `nmp_content`
//! `tokenize_with_kind` and whose `embeds` are resolved through the real
//! `RenderContext` recursion guard (see `crate::embed_store`).

pub mod edge;
pub mod hashtags;
pub mod links;
pub mod lists;
pub mod media;
pub mod mentions;
pub mod quotes;
pub mod text;

use nmp_content::{tokenize_with_kind, RenderMode};
use nmp_core::substrate::SignedEvent;

use crate::dto::{ScenarioDto, SignedEventJson};
use crate::embed_store::EmbedStore;
use crate::project::project_tree;

/// JSON form of a signed fixture event.
pub(crate) fn ev_json(ev: &SignedEvent) -> SignedEventJson {
    SignedEventJson {
        id: ev.id.clone(),
        pubkey: ev.unsigned.pubkey.clone(),
        created_at: ev.unsigned.created_at,
        kind: ev.unsigned.kind,
        tags: ev.unsigned.tags.clone(),
        content: ev.unsigned.content.clone(),
        sig: ev.sig.clone(),
    }
}

/// Assemble a scenario: tokenize the primary event with the real path,
/// project to DTO, and resolve every embed against the relay-free store.
pub(crate) fn scenario(
    id: &str,
    category: &str,
    title: &str,
    exercises: &str,
    primary: &SignedEvent,
    extra_events: Vec<SignedEvent>,
    store: &EmbedStore,
) -> ScenarioDto {
    let tree = tokenize_with_kind(
        &primary.unsigned.content,
        &primary.unsigned.tags,
        RenderMode::Auto,
        primary.unsigned.kind,
    );
    let rendered = project_tree(&tree);
    let embeds = store.resolve_all(&rendered);

    let mut events = vec![ev_json(primary)];
    events.extend(extra_events.iter().map(ev_json));

    ScenarioDto {
        id: id.to_string(),
        category: category.to_string(),
        title: title.to_string(),
        exercises: exercises.to_string(),
        events,
        rendered,
        embeds,
    }
}

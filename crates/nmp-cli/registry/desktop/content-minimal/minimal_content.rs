use std::collections::BTreeMap;

use iced::widget::row;
use iced::Element;

use nmp_gallery_tui::content_render_data::ContentRenderData;
use nmp_gallery_tui::content_tree_wire::{ContentTreeWire, WireNode};

use super::content_core::{pill, short_id};
use super::mention_chip::NostrMentionChip;

pub struct NostrMinimalContent<'a> {
    tree: &'a ContentTreeWire,
    render_data: Option<&'a ContentRenderData>,
    profile_labels: BTreeMap<String, String>,
}

impl<'a> NostrMinimalContent<'a> {
    #[must_use]
    pub fn new(tree: &'a ContentTreeWire) -> Self {
        Self {
            tree,
            render_data: None,
            profile_labels: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn render_data(mut self, render_data: Option<&'a ContentRenderData>) -> Self {
        self.render_data = render_data;
        self
    }

    #[must_use]
    pub fn profile_labels(mut self, profile_labels: BTreeMap<String, String>) -> Self {
        self.profile_labels = profile_labels;
        self
    }

    pub fn into_element<Message: 'static>(self) -> Element<'a, Message> {
        let mut out = row![].spacing(6);
        for node in &self.tree.nodes {
            match node {
                WireNode::Mention(uri) => {
                    let mut chip = NostrMentionChip::new(uri)
                        .profile(self.render_data.and_then(|data| data.profile_for(uri)));
                    if let Some(label) = self.profile_labels.get(&uri.primary_id) {
                        chip = chip.label(label.clone());
                    }
                    out = out.push(chip.into_element());
                }
                WireNode::Text(value) if !value.trim().is_empty() => {
                    out = out.push(pill(value.clone()));
                }
                WireNode::EventRef(uri) => {
                    out = out.push(pill(format!("event {}", short_id(&uri.primary_id))));
                }
                _ => {}
            }
        }
        out.into()
    }
}

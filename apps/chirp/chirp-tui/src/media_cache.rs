use std::{
    collections::HashMap,
    io::{IsTerminal, Read},
    sync::mpsc::Sender,
    thread,
    time::Duration,
};

use image::DynamicImage;
use ratatui::layout::Size;
use ratatui_image::{picker::Picker, picker::ProtocolType, protocol::Protocol, Resize};

use crate::{app::AppState, timeline::TimelineRow};

const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;

pub struct MediaCache {
    picker: Option<Picker>,
    entries: HashMap<String, MediaEntry>,
}

pub enum MediaFetch {
    Loaded { url: String, image: DynamicImage },
    Failed { url: String },
}

enum MediaEntry {
    Loading,
    Ready(Protocol),
    Failed,
}

impl MediaCache {
    pub fn new() -> Self {
        Self {
            picker: None,
            entries: HashMap::new(),
        }
    }

    pub fn sync_urls(&mut self, urls: Vec<String>, tx: Sender<MediaFetch>) {
        for url in urls {
            if self.entries.contains_key(&url) || !is_fetchable_url(&url) {
                continue;
            }
            self.entries.insert(url.clone(), MediaEntry::Loading);
            spawn_fetch(url, tx.clone());
        }
    }

    pub fn apply_fetch(&mut self, event: MediaFetch) {
        match event {
            MediaFetch::Loaded { url, image } => {
                let protocol = self
                    .picker()
                    .new_protocol(image, Size::new(30, 8), Resize::Fit(None))
                    .ok();
                self.entries
                    .insert(url, protocol.map_or(MediaEntry::Failed, MediaEntry::Ready));
            }
            MediaFetch::Failed { url } => {
                self.entries.insert(url, MediaEntry::Failed);
            }
        }
    }

    pub fn image_refs(&self) -> Vec<(&str, &Protocol)> {
        self.entries
            .iter()
            .filter_map(|(url, entry)| match entry {
                MediaEntry::Ready(protocol) => Some((url.as_str(), protocol)),
                _ => None,
            })
            .collect()
    }

    fn picker(&mut self) -> &mut Picker {
        self.picker.get_or_insert_with(new_picker)
    }
}

impl Default for MediaCache {
    fn default() -> Self {
        Self::new()
    }
}

pub fn visible_media_urls(state: &AppState) -> Vec<String> {
    let mut urls = Vec::new();
    let Some(root_idx) = selected_root_index(&state.rows, state.selected) else {
        return urls;
    };
    collect_row_urls(&mut urls, &state.rows[root_idx]);
    for row in state
        .rows
        .iter()
        .skip(root_idx + 1)
        .take_while(|row| row.depth > 0)
    {
        collect_row_urls(&mut urls, row);
    }
    urls
}

fn collect_row_urls(out: &mut Vec<String>, row: &TimelineRow) {
    for url in row.media_urls() {
        if !out.iter().any(|existing| existing == &url) {
            out.push(url);
        }
    }
}

fn selected_root_index(rows: &[TimelineRow], selected: usize) -> Option<usize> {
    if rows.is_empty() {
        return None;
    }
    let selected = selected.min(rows.len().saturating_sub(1));
    (0..=selected)
        .rev()
        .find(|idx| rows[*idx].depth == 0)
        .or(Some(0))
}

fn spawn_fetch(url: String, tx: Sender<MediaFetch>) {
    thread::spawn(move || {
        let event = fetch_image(&url)
            .map(|image| MediaFetch::Loaded {
                url: url.clone(),
                image,
            })
            .unwrap_or(MediaFetch::Failed { url });
        let _ = tx.send(event);
    });
}

fn fetch_image(url: &str) -> Option<DynamicImage> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(12))
        .build();
    let response = agent.get(url).call().ok()?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_IMAGE_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    image::load_from_memory(&bytes).ok()
}

fn new_picker() -> Picker {
    let mut picker = if std::io::stdout().is_terminal() {
        Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks())
    } else {
        Picker::halfblocks()
    };
    if std::env::var("ITERM_SESSION_ID").is_ok() {
        picker.set_protocol_type(ProtocolType::Iterm2);
    }
    picker
}

fn is_fetchable_url(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::nostr_content::{
        content_render_data::ContentRenderData, content_tree_wire::ContentTreeWire,
    };
    use crate::ui::nostr_user::profile_wire::ProfileWire;

    #[test]
    fn visible_media_urls_tracks_selected_thread_only() {
        let mut state = AppState::default();
        state.rows = vec![
            row(0, &["https://example.com/root-a.jpg"]),
            row(1, &["https://example.com/reply.jpg"]),
            row(0, &["https://example.com/other-root.jpg"]),
        ];
        state.selected = 1;

        assert_eq!(
            visible_media_urls(&state),
            vec![
                "https://example.com/root-a.jpg".to_string(),
                "https://example.com/reply.jpg".to_string(),
            ]
        );
    }

    #[test]
    fn loaded_image_becomes_renderable_protocol() {
        let mut cache = MediaCache::new();
        cache.apply_fetch(MediaFetch::Loaded {
            url: "https://example.com/image.png".to_string(),
            image: DynamicImage::new_rgb8(2, 2),
        });
        assert_eq!(cache.image_refs().len(), 1);
    }

    fn row(depth: usize, urls: &[&str]) -> TimelineRow {
        let pubkey = "a".repeat(64);
        TimelineRow {
            id: format!("row-{depth}-{}", urls.len()),
            author_profile: ProfileWire {
                display_name: Some("alice".to_string()),
                pubkey: pubkey.clone(),
                npub: String::new(),
                npub_short: String::new(),
                about: None,
                picture_url: None,
                nip05: None,
            },
            author_pubkey: pubkey,
            content: String::new(),
            created_at: 1,
            depth,
            has_gap: false,
            thread_attribution: Vec::new(),
            relation_counts: Default::default(),
            relay_provenance: Vec::new(),
            content_tree: Some(media_tree(urls)),
            content_render: ContentRenderData::default(),
            mention_pubkeys: Vec::new(),
            repost: None,
        }
    }

    fn media_tree(urls: &[&str]) -> ContentTreeWire {
        let value = serde_json::json!({
            "nodes": [{
                "kind": "media",
                "media_kind": "image",
                "urls": urls,
            }],
            "roots": [0],
            "mode": "plaintext",
        });
        ContentTreeWire::from_value(&value).expect("valid media tree")
    }
}

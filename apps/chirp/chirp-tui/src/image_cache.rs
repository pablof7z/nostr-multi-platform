use std::collections::{BTreeSet, HashMap};
use std::io::Read;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use image::DynamicImage;
use ratatui::layout::Size;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::Protocol;
use ratatui_image::Resize;

use crate::app::AppState;
use crate::timeline_content::TimelineMediaKind;

const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
const PREVIEW_COLS: u16 = 52;
const PREVIEW_ROWS: u16 = 14;

pub enum ImageEvent {
    Loaded { url: String, image: DynamicImage },
    Failed { url: String },
}

#[derive(Default)]
pub struct ImageCache {
    picker: Option<Picker>,
    protocols: HashMap<String, Protocol>,
    pending: BTreeSet<String>,
    failed: BTreeSet<String>,
}

impl ImageCache {
    pub fn enabled() -> Self {
        Self {
            picker: Some(Picker::halfblocks()),
            protocols: HashMap::new(),
            pending: BTreeSet::new(),
            failed: BTreeSet::new(),
        }
    }

    pub fn disabled() -> Self {
        Self::default()
    }

    pub fn protocol(&self, url: &str) -> Option<&Protocol> {
        self.protocols.get(url)
    }

    pub fn absorb(&mut self, event: ImageEvent) {
        match event {
            ImageEvent::Loaded { url, image } => {
                self.pending.remove(&url);
                match self.protocol_for_image(image) {
                    Ok(protocol) => {
                        self.failed.remove(&url);
                        self.protocols.insert(url, protocol);
                    }
                    Err(_) => {
                        self.failed.insert(url);
                    }
                }
            }
            ImageEvent::Failed { url } => {
                self.pending.remove(&url);
                self.failed.insert(url);
            }
        }
    }

    pub fn request_selected(&mut self, state: &AppState, tx: &mpsc::Sender<ImageEvent>) {
        let Some(row) = state.selected_row() else {
            return;
        };
        for media in row
            .media
            .iter()
            .filter(|m| m.kind == TimelineMediaKind::Image)
        {
            if self.protocols.contains_key(&media.url)
                || self.pending.contains(&media.url)
                || self.failed.contains(&media.url)
            {
                continue;
            }
            self.pending.insert(media.url.clone());
            spawn_fetch(media.url.clone(), tx.clone());
            break;
        }
    }

    fn protocol_for_image(&mut self, image: DynamicImage) -> Result<Protocol, String> {
        let Some(picker) = self.picker.as_mut() else {
            return Err("image cache disabled".to_string());
        };
        picker
            .new_protocol(
                image,
                Size::new(PREVIEW_COLS, PREVIEW_ROWS),
                Resize::Fit(None),
            )
            .map_err(|err| err.to_string())
    }
}

fn spawn_fetch(url: String, tx: mpsc::Sender<ImageEvent>) {
    thread::spawn(move || {
        let event = match fetch_image(&url) {
            Ok(image) => ImageEvent::Loaded {
                url: url.clone(),
                image,
            },
            Err(_) => ImageEvent::Failed { url: url.clone() },
        };
        let _ = tx.send(event);
    });
}

fn fetch_image(url: &str) -> Result<DynamicImage, String> {
    let response = ureq::get(url)
        .timeout(Duration::from_secs(8))
        .call()
        .map_err(|err| err.to_string())?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_IMAGE_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|err| err.to_string())?;
    image::load_from_memory(&bytes).map_err(|err| err.to_string())
}

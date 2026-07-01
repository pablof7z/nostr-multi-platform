use std::collections::BTreeMap;

use iced::widget::{column, container, image, row, text};
use iced::{Border, Element, Length};

use super::content_core::{short_id, BORDER_COLOR, MUTED, SURFACE_2, TEXT};

pub struct NostrMediaGrid<'a> {
    urls: Vec<String>,
    image_handles: &'a BTreeMap<String, image::Handle>,
}

impl<'a> NostrMediaGrid<'a> {
    #[must_use]
    pub fn new(urls: &[String]) -> Self {
        Self {
            urls: urls.to_vec(),
            image_handles: &EMPTY_HANDLES,
        }
    }

    #[must_use]
    pub fn image_handles(mut self, image_handles: &'a BTreeMap<String, image::Handle>) -> Self {
        self.image_handles = image_handles;
        self
    }

    pub fn into_element<Message: 'static>(self) -> Element<'a, Message> {
        if self.urls.is_empty() {
            return text("Waiting for relay-backed media...")
                .size(13)
                .style(|_| text::Style { color: Some(MUTED) })
                .into();
        }

        let mut rows = column![].spacing(8);
        for chunk in self.urls.chunks(2) {
            let mut cells = row![].spacing(8);
            for url in chunk {
                cells = cells.push(media_cell(
                    url.clone(),
                    self.image_handles.get(url).cloned(),
                ));
            }
            rows = rows.push(cells);
        }
        rows.into()
    }
}

static EMPTY_HANDLES: BTreeMap<String, image::Handle> = BTreeMap::new();

fn media_cell<Message: 'static>(
    url: String,
    handle: Option<image::Handle>,
) -> Element<'static, Message> {
    let body: Element<'static, Message> = if let Some(handle) = handle {
        image(handle)
            .width(Length::Fill)
            .height(Length::Fixed(132.0))
            .into()
    } else {
        container(
            column![
                text("image")
                    .size(12)
                    .style(|_| text::Style { color: Some(TEXT) }),
                text(short_id(&url))
                    .size(11)
                    .style(|_| text::Style { color: Some(MUTED) })
            ]
            .spacing(4),
        )
        .height(Length::Fixed(132.0))
        .center(Length::Fill)
        .into()
    };

    container(body)
        .width(Length::FillPortion(1))
        .clip(true)
        .style(|_| container::Style {
            background: Some(iced::Background::Color(SURFACE_2)),
            border: Border {
                color: BORDER_COLOR,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        })
        .into()
}

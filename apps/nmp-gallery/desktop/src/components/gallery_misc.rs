use iced::widget::{column, text};
use iced::{Color, Element};

const MUTED_TEXT: Color = Color {
    r: 0.580,
    g: 0.639,
    b: 0.722,
    a: 1.0,
};

pub fn relay_list<'a, Message: 'a>() -> Element<'a, Message> {
    let refs = nmp_app_gallery::showcase::references();
    let mut col = column![text("Configured showcase relays").size(14)].spacing(6);
    for relay in &refs.relays {
        col = col.push(text(format!("{} [{}]", relay.url, relay.role)).size(13));
    }
    col.into()
}

pub fn login_block<'a, Message: 'a>() -> Element<'a, Message> {
    column![
        text("NostrLoginBlock").size(14),
        text("Signer detection and manual key-entry login UI.")
            .size(13)
            .style(|_| text::Style {
                color: Some(MUTED_TEXT)
            }),
    ]
    .spacing(6)
    .into()
}

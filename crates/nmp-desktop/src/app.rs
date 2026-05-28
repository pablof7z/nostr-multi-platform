//! iced application shell.
//!
//! Pure projection of the latest kernel [`Snapshot`] (D7: the UI owns no state
//! beyond the snapshot + transient input buffers). All mutations go back into
//! the kernel as `ActorCommand`s via the command sender stored on first
//! `BridgeReady` message.

use std::sync::mpsc::Sender;

use iced::widget::{
    button, column, container, row, scrollable, text, text_editor, text_input, Space,
};
use iced::{Color, Element, Length, Theme};

use nmp_core::testing::ActorCommand;

use crate::bridge;
use crate::message::Message;
use crate::render::{effective_content, hex_color};
use crate::snapshot::Snapshot;

pub struct DesktopApp {
    tx: Option<Sender<ActorCommand>>,
    snapshot: Snapshot,
    compose: iced::widget::text_editor::Content,
    nsec: String,
}

impl DesktopApp {
    #[must_use]
    pub fn new() -> Self {
        Self {
            tx: None,
            snapshot: Snapshot::default(),
            compose: iced::widget::text_editor::Content::new(),
            nsec: String::new(),
        }
    }
}

pub fn update(app: &mut DesktopApp, message: Message) {
    match message {
        Message::BridgeReady(tx) => {
            app.tx = Some(tx);
        }
        Message::SnapshotUpdated(snap) => {
            app.snapshot = snap;
        }
        Message::ComposeAction(action) => {
            app.compose.perform(action);
        }
        Message::NsecChanged(val) => {
            app.nsec = val;
        }
        Message::Publish => {
            if let Some(tx) = &app.tx {
                bridge::publish_note(tx, app.compose.text().trim().to_string());
                app.compose = iced::widget::text_editor::Content::new();
            }
        }
        Message::CreateAccount => {
            if let Some(tx) = &app.tx {
                bridge::create_account(
                    tx,
                    [("name".to_string(), "New User".to_string())].into(),
                    vec![
                        (
                            "wss://relay.primal.net".to_string(),
                            "both,indexer".to_string(),
                        ),
                        (
                            "wss://purplepag.es".to_string(),
                            "indexer".to_string(),
                        ),
                    ],
                );
                bridge::open_timeline(tx);
            }
        }
        Message::SignIn => {
            if let Some(tx) = &app.tx {
                if !app.nsec.trim().is_empty() {
                    bridge::sign_in_nsec(tx, app.nsec.trim().to_string());
                    app.nsec.clear();
                    bridge::open_timeline(tx);
                }
            }
        }
        Message::OpenTimeline => {
            if let Some(tx) = &app.tx {
                bridge::open_timeline(tx);
            }
        }
        Message::OpenUrl(url) => {
            let _ = webbrowser::open(&url);
        }
    }
}

pub fn view(app: &DesktopApp) -> Element<'_, Message> {
    column![
        status_bar(app),
        scrollable(timeline(app)).height(Length::Fill),
        compose_bar(app),
    ]
    .into()
}

fn status_bar(app: &DesktopApp) -> Element<'_, Message> {
    let snap = &app.snapshot;
    let dot = if snap.running { "🟢" } else { "⚪️" };

    let mut status = row![text(format!("NMP  {dot} rev {}", snap.rev)).size(16)].spacing(8);

    for r in &snap.relay_statuses {
        let connected = r.connection.eq_ignore_ascii_case("connected")
            || r.connection.eq_ignore_ascii_case("ready");
        let color = if connected {
            Color::from_rgb8(74, 222, 128)
        } else {
            Color::from_rgb8(248, 113, 113)
        };
        status = status.push(
            text(format!("{} {}", r.role, r.connection))
                .size(12)
                .style(move |_theme: &Theme| text::Style { color: Some(color) }),
        );
    }

    status = status.push(
        text(format!(
            "{} notes · {} rx · {} visible",
            snap.metrics.note_events,
            snap.metrics.events_rx,
            snap.metrics.visible_items
        ))
        .size(12),
    );

    container(status.padding(8)).style(|_theme: &Theme| container::Style {
        background: Some(iced::Background::Color(Color::from_rgb8(30, 30, 30))),
        ..Default::default()
    }).into()
}

fn compose_bar(app: &DesktopApp) -> Element<'_, Message> {
    let snap = &app.snapshot;
    let signed_in = snap.active_account.is_some();

    let mut col = column![].spacing(8).padding(12);

    if !signed_in {
        let sign_in_row = row![
            text("Sign in to publish:").size(14),
            button("Create new account")
                .on_press(Message::CreateAccount),
            text_input("nsec1… or hex secret", &app.nsec)
                .on_input(Message::NsecChanged)
                .secure(true),
            button("Sign in")
                .on_press(Message::SignIn),
        ]
        .spacing(8);
        col = col.push(sign_in_row);
    }

    if let Some(err) = &snap.last_error_toast {
        col = col.push(
            text(err)
                .size(12)
                .style(|_theme: &Theme| text::Style {
                    color: Some(Color::from_rgb8(248, 113, 113)),
                }),
        );
    }

    let hint = if signed_in {
        "Write a note…"
    } else {
        "Write a note (sign in first to publish)…"
    };

    col = col.push(
        text_editor(&app.compose,
        )
        .on_action(Message::ComposeAction)
        .placeholder(hint)
        .padding(8),
    );

    let can_send = signed_in && !app.compose.text().trim().is_empty();
    let mut action_row = row![Space::new().width(Length::Fill)];

    if let Some(name) = snap.profile.display_name.as_deref() {
        if !name.is_empty() {
            action_row = action_row.push(text(format!("as {name}")).size(12));
        }
    } else if !snap.profile.pubkey.is_empty() {
        action_row = action_row.push(
            text(format!(
                "as {}",
                nmp_core::display::short_npub(&snap.profile.pubkey)
            ))
            .size(12),
        );
    }

    action_row = action_row.push(
        button("Publish")
            .on_press_maybe(if can_send { Some(Message::Publish) } else { None }),
    );

    col = col.push(action_row);

    container(col).style(|_theme: &Theme| container::Style {
        background: Some(iced::Background::Color(Color::from_rgb8(30, 30, 30))),
        ..Default::default()
    }).into()
}

fn timeline(app: &DesktopApp) -> Element<'_, Message> {
    let snap = &app.snapshot;

    if snap.items.is_empty() {
        return column![
            Space::new().height(40.0),
            text("Connecting to wss://relay.primal.net…")
                .size(15)
                .style(|_theme: &Theme| text::Style {
                    color: Some(Color::from_rgb8(160, 160, 160)),
                }),
            text("Live seed timeline will appear here.")
                .size(14)
                .style(|_theme: &Theme| text::Style {
                    color: Some(Color::from_rgb8(160, 160, 160)),
                }),
        ]
        .align_x(iced::alignment::Horizontal::Center)
        .into();
    }

    let mut items = column![].spacing(6).padding(8);
    for item in &snap.items {
        items = items.push(note_card(item));
    }
    items.into()
}

fn note_card(item: &crate::snapshot::TimelineItem) -> Element<'_, Message> {
    let author_display = nmp_core::display::short_npub(&item.author_pubkey);
    let initials = nmp_core::display::avatar_initials(
        &nmp_core::display::to_npub(&item.author_pubkey),
    );
    let color = nmp_core::display::avatar_color_hex(&item.author_pubkey,
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let created_at_display = nmp_core::display::format_ago_secs(now, item.created_at);

    let mut meta = row![
        text(author_display).size(13).font(iced::Font { weight: iced::font::Weight::Bold, ..iced::Font::DEFAULT }),
        Space::new().width(Length::Fill),
    ];
    if item.relay_count > 1 {
        meta = meta.push(
            text(format!("·{}×", item.relay_count))
                .size(11)
                .style(|_theme: &Theme| text::Style {
                    color: Some(Color::from_rgb8(160, 160, 160)),
                }),
        );
    }
    meta = meta.push(
        text(created_at_display)
            .size(11)
            .style(|_theme: &Theme| text::Style {
                color: Some(Color::from_rgb8(160, 160, 160)),
            }),
    );

    let mut body_col = column![meta].spacing(4);

    let (text_content, is_repost) = effective_content(&item.content);
    if is_repost {
        body_col = body_col.push(
            text("↩ repost")
                .size(11)
                .style(|_theme: &Theme| text::Style {
                    color: Some(Color::from_rgb8(148, 163, 184)),
                }),
        );
    }
    if !text_content.is_empty() {
        body_col = body_col.push(crate::render::note_body(text_content.into_owned()));
    }

    let row_content = row![
        avatar(&initials, &color),
        body_col,
    ]
    .spacing(8)
    .align_y(iced::alignment::Vertical::Top);

    container(row_content.padding(8))
        .style(|_theme: &Theme| container::Style {
            background: Some(iced::Background::Color(Color::from_rgb8(40, 40, 40))),
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

fn avatar(initials: &str, color_hex: &str) -> Element<'static, Message> {
    let initials_owned = initials.to_string();
    let size = 36.0;
    let color = hex_color(color_hex);
    container(
        text(initials_owned)
            .size(size * 0.4)
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center),
    )
    .width(Length::Fixed(size))
    .height(Length::Fixed(size))
    .style(move |_theme: &Theme| container::Style {
        background: Some(iced::Background::Color(color)),
        border: iced::Border {
            radius: (size / 2.0).into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

//! egui application shell for Chirp Desktop.
//!
//! Renders the latest kernel [`Snapshot`] with left-sidebar navigation,
//! a central content area (timeline, thread, author, or settings),
//! a top status bar, and a bottom compose bar.

use std::sync::{Arc, Mutex};

use eframe::App;
use egui::{
    Align, CentralPanel, Color32, Frame, Layout, RichText, ScrollArea, SidePanel, TextEdit,
    TopBottomPanel, Ui,
};

use std::collections::HashMap;

use crate::bridge::AppRuntime;
use crate::render::{hex_color, note_body};
use crate::snapshot::{
    ActionStageRow, ModularTimelineSnapshot, ProfileCard, Snapshot, TimelineEventCard,
};
use nmp_core::tags::Nip10Refs;
use nmp_nip01::NoteRecord;

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum AppTab {
    Home,
    Thread(String),
    Author(String),
    Dms,
    Settings,
    Diagnostics,
    Outbox,
}

pub struct DesktopApp {
    pub(crate) bridge: AppRuntime,
    pub(crate) latest: Arc<Mutex<Option<Snapshot>>>,
    pub(crate) tab: AppTab,
    pub(crate) compose: String,
    pub(crate) reply_to: Option<NoteRecord>,
    pub(crate) selected_dm_pubkey: Option<String>,
    pub(crate) new_dm_pubkey: String,
    pub(crate) dm_compose: String,
    pub(crate) nsec_input: String,
    pub(crate) bunker_relay_input: String,
    pub(crate) bunker_uri: Option<String>,
    pub(crate) new_relay_url: String,
    pub(crate) new_relay_role: String,
    pub(crate) edit_display_name: String,
    pub(crate) edit_about: String,
    pub(crate) edit_picture: String,
    pub(crate) show_edit_profile: bool,
    pub(crate) nwc_input: String,
}

impl DesktopApp {
    #[must_use]
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (bridge, rx) = AppRuntime::new().expect("Failed to boot Chirp kernel");
        let latest: Arc<Mutex<Option<Snapshot>>> = Arc::new(Mutex::new(None));

        let reader_latest = Arc::clone(&latest);
        let egui_ctx = cc.egui_ctx.clone();
        std::thread::spawn(move || {
            for event in rx {
                // PR-B (#991/#979): typed-first decode. The `payload:Value`
                // blob is no longer emitted; every field is read from the
                // strongly-typed `SnapshotEnvelope` (rev / running / metrics /
                // relay_statuses / last_error_toast) and the per-projection
                // typed sidecars. Each projection the shell renders is decoded
                // from its sidecar and re-materialised as a `serde_json::Value`
                // (built via `serde_json::json!`, since the `snapshot::*`
                // payload structs are `Deserialize`-only) so the existing
                // `snap.projection::<T>(key)` read sites keep working unchanged.
                let Some(snap) = crate::snapshot_decode::decode_snapshot_typed(&event.payload) else {
                    continue;
                };
                if let Ok(mut slot) = reader_latest.lock() {
                    *slot = Some(snap);
                }
                egui_ctx.request_repaint();
            }
        });

        Self {
            bridge,
            latest,
            tab: AppTab::Home,
            compose: String::new(),
            reply_to: None,
            selected_dm_pubkey: None,
            new_dm_pubkey: String::new(),
            dm_compose: String::new(),
            nsec_input: String::new(),
            bunker_relay_input: String::new(),
            bunker_uri: None,
            new_relay_url: String::new(),
            new_relay_role: "both".to_string(),
            edit_display_name: String::new(),
            edit_about: String::new(),
            edit_picture: String::new(),
            show_edit_profile: false,
            nwc_input: String::new(),
        }
    }

    fn snapshot(&self) -> Option<Snapshot> {
        self.latest.lock().ok().and_then(|s| s.clone())
    }
}

// ---------------------------------------------------------------------------
// egui App trait
// ---------------------------------------------------------------------------

impl App for DesktopApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let snap = self.snapshot().unwrap_or_default();

        self.status_bar(ctx, &snap);
        self.sidebar(ctx, &snap);
        self.content(ctx, &snap);

        if matches!(self.tab, AppTab::Home | AppTab::Thread(_)) || self.reply_to.is_some() {
            self.compose_bar(ctx, &snap);
        }
    }
}

// ---------------------------------------------------------------------------
// Panels
// ---------------------------------------------------------------------------

impl DesktopApp {
    fn status_bar(&self, ctx: &egui::Context, snap: &Snapshot) {
        TopBottomPanel::top("status").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading("Chirp");
                ui.separator();
                let dot = if snap.running { "🟢" } else { "⚪️" };
                ui.label(format!("{dot} rev {}", snap.rev));
                ui.separator();
                for r in &snap.relay_statuses {
                    let connected = r.connection.eq_ignore_ascii_case("connected")
                        || r.connection.eq_ignore_ascii_case("ready");
                    let color = if connected {
                        Color32::from_rgb(74, 222, 128)
                    } else {
                        Color32::from_rgb(248, 113, 113)
                    };
                    ui.label(RichText::new(format!("{} {}", r.role, r.connection)).color(color))
                        .on_hover_text(&r.relay_url);
                    ui.separator();
                }
                ui.label(format!(
                    "{} notes · {} rx · {} visible",
                    snap.metrics.note_events, snap.metrics.events_rx, snap.metrics.visible_items
                ));
            });
            ui.add_space(4.0);
        });
    }

    fn sidebar(&mut self, ctx: &egui::Context, snap: &Snapshot) {
        SidePanel::left("sidebar")
            .resizable(false)
            .width_range(140.0..=180.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("Chirp").size(18.0).strong());
                });
                ui.add_space(12.0);

                let current_tab = self.tab.clone();

                if ui
                    .selectable_label(matches!(current_tab, AppTab::Home), "🏠  Home")
                    .clicked()
                {
                    self.tab = AppTab::Home;
                    self.bridge.open_timeline();
                }
                if ui
                    .selectable_label(matches!(current_tab, AppTab::Author(_)), "👤  Profile")
                    .clicked()
                {
                    if let Some(ref pk) = snap.active_account {
                        self.tab = AppTab::Author(pk.clone());
                        self.bridge.open_author(pk);
                    }
                }
                if ui
                    .selectable_label(matches!(current_tab, AppTab::Dms), "💬  DMs")
                    .clicked()
                {
                    self.tab = AppTab::Dms;
                }
                if ui
                    .selectable_label(matches!(current_tab, AppTab::Settings), "⚙️  Settings")
                    .clicked()
                {
                    self.tab = AppTab::Settings;
                }
                if ui
                    .selectable_label(
                        matches!(current_tab, AppTab::Diagnostics),
                        "📊  Diagnostics",
                    )
                    .clicked()
                {
                    self.tab = AppTab::Diagnostics;
                }
                if ui
                    .selectable_label(matches!(current_tab, AppTab::Outbox), "📤  Outbox")
                    .clicked()
                {
                    self.tab = AppTab::Outbox;
                }

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);

                // Active account mini-card
                if let Some(ref pk) = snap.active_account {
                    // ADR-0032 / V-115: `profile.npub` is always empty; derive
                    // the fallback from the raw pubkey on the host side.
                    let npub_fallback = nmp_core::display::to_npub(pk);
                    let name = snap
                        .profile
                        .display_name
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .unwrap_or(npub_fallback.as_str());
                    ui.label(RichText::new(name).strong().small());
                    ui.label(
                        RichText::new(nmp_core::display::short_npub(pk))
                            .small()
                            .weak(),
                    );
                } else {
                    ui.label(RichText::new("No account").small().weak());
                }
            });
    }

    fn content(&mut self, ctx: &egui::Context, snap: &Snapshot) {
        let tab = self.tab.clone();
        CentralPanel::default().show(ctx, |ui| match tab {
            AppTab::Home => self.timeline(ui, snap),
            AppTab::Thread(ref event_id) => {
                // V-112 (ADR-0042): read from flat-feed projection instead of deleted thread_view.
                let key = format!("nmp.feed.thread.{event_id}");
                let feed: Option<ModularTimelineSnapshot> = snap.projection(&key);
                self.thread_view(ui, snap, event_id, feed);
            }
            AppTab::Author(ref pubkey) => {
                // V-112 (ADR-0042): read from flat-feed projection instead of deleted author_view.
                let key = format!("nmp.feed.author.{pubkey}");
                let feed: Option<ModularTimelineSnapshot> = snap.projection(&key);
                let profiles: HashMap<String, ProfileCard> =
                    snap.projection("resolved_profiles").unwrap_or_default();
                self.author_view(ui, snap, pubkey, feed, profiles);
            }
            AppTab::Dms => crate::dm_panel::show(self, ui, snap),
            AppTab::Settings => self.settings_view(ui, snap),
            AppTab::Diagnostics => self.diagnostics_panel(ui, snap),
            AppTab::Outbox => self.outbox_panel(ui, snap),
        });
    }

    fn compose_bar(&mut self, ctx: &egui::Context, snap: &Snapshot) {
        TopBottomPanel::bottom("compose").show(ctx, |ui| {
            ui.add_space(6.0);

            let signed_in = snap.active_account.is_some();
            let explicit_reply = self.reply_to.clone();
            let thread_reply = match &self.tab {
                AppTab::Thread(event_id) => thread_reply_target(snap, event_id),
                _ => None,
            };
            let reply_target = explicit_reply.as_ref().or(thread_reply.as_ref());

            if let Some(err) = &snap.last_error_toast {
                ui.colored_label(Color32::from_rgb(248, 113, 113), err);
            }

            if let Some(target) = reply_target {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "Replying to {}",
                            nmp_core::display::short_npub(&target.author)
                        ))
                        .small()
                        .weak(),
                    );
                    if explicit_reply.is_some() && ui.small_button("Cancel").clicked() {
                        self.reply_to = None;
                    }
                });
            }

            ui.horizontal(|ui| {
                let hint = if reply_target.is_some() {
                    "Write a reply…"
                } else if signed_in {
                    "Write a note…"
                } else {
                    "Write a note (sign in first to publish)…"
                };
                ui.add(
                    TextEdit::multiline(&mut self.compose)
                        .hint_text(hint)
                        .desired_rows(2)
                        .desired_width(f32::INFINITY),
                );
            });
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let can_send = signed_in && !self.compose.trim().is_empty();
                    let label = if reply_target.is_some() {
                        "Reply"
                    } else {
                        "Publish"
                    };
                    if ui
                        .add_enabled(can_send, egui::Button::new(label))
                        .clicked()
                    {
                        let _ = self
                            .bridge
                            .publish_note(self.compose.trim(), reply_target);
                        self.compose.clear();
                        self.reply_to = None;
                    }
                    if let Some(name) = snap.profile.display_name.as_deref() {
                        if !name.is_empty() {
                            ui.label(RichText::new(format!("as {name}")).weak());
                        }
                    } else if !snap.profile.pubkey.is_empty() {
                        ui.label(
                            RichText::new(format!(
                                "as {}",
                                nmp_core::display::short_npub(&snap.profile.pubkey)
                            ))
                            .weak(),
                        );
                    }
                });
            });
            ui.add_space(6.0);
        });
    }
}

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

impl DesktopApp {
    fn timeline(&mut self, ui: &mut Ui, snap: &Snapshot) {
        let feed: ModularTimelineSnapshot =
            snap.projection("nmp.feed.home").unwrap_or_default();
        // Pre-merged display-name map keyed by hex pubkey (kernel projection).
        let profiles: HashMap<String, ProfileCard> =
            snap.projection("resolved_profiles").unwrap_or_default();

        if feed.cards.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label(RichText::new("Connecting to relays…").size(15.0).weak());
                ui.label(RichText::new("Live timeline will appear here.").weak());
            });
            return;
        }

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for entry in &feed.cards {
                    feed_card(
                        ui,
                        &entry.card,
                        &profiles,
                        &snap.embeds,
                        &mut self.tab,
                        &self.bridge,
                        &mut self.reply_to,
                    );
                    ui.add_space(6.0);
                }
            });
    }

    fn thread_view(
        &mut self,
        ui: &mut Ui,
        snap: &Snapshot,
        event_id: &str,
        feed: Option<ModularTimelineSnapshot>,
    ) {
        let eid = event_id.to_string();
        let profiles: HashMap<String, ProfileCard> =
            snap.projection("resolved_profiles").unwrap_or_default();
        ui.horizontal(|ui| {
            if ui.button("← Back").clicked() {
                self.tab = AppTab::Home;
                self.bridge.close_thread(&eid);
            }
            ui.label(RichText::new("Thread").strong());
        });
        ui.separator();

        // V-112 (ADR-0042): thread_view projection deleted; items come from flat feed.
        let Some(thread_feed) = feed else {
            ui.label("Loading thread…");
            return;
        };

        ui.add_space(4.0);

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for entry in &thread_feed.cards {
                    feed_card(
                        ui,
                        &entry.card,
                        &profiles,
                        &snap.embeds,
                        &mut self.tab,
                        &self.bridge,
                        &mut self.reply_to,
                    );
                    ui.add_space(4.0);
                }
            });
    }

    fn author_view(
        &mut self,
        ui: &mut Ui,
        snap: &Snapshot,
        pubkey: &str,
        feed: Option<ModularTimelineSnapshot>,
        profiles: HashMap<String, ProfileCard>,
    ) {
        let pk = pubkey.to_string();
        ui.horizontal(|ui| {
            if ui.button("← Back").clicked() {
                self.tab = AppTab::Home;
                self.bridge.close_author(&pk);
            }
            ui.label(RichText::new("Profile").strong());
        });
        ui.separator();

        // V-112 (ADR-0042): author_view projection deleted; items come from flat feed.
        // Profile header uses resolved_profiles (kernel-claimed via claim_profile).
        let initials = nmp_core::display::avatar_initials(&nmp_core::display::to_npub(pubkey));
        let color = nmp_core::display::avatar_color_hex(pubkey);
        let profile = profiles.get(pubkey).cloned().unwrap_or_default();
        ui.horizontal(|ui| {
            avatar(ui, &initials, &color);
            ui.add_space(8.0);
            ui.vertical(|ui| {
                let name = profile
                    .display_name
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("(no name)");
                ui.label(RichText::new(name).size(16.0).strong());
                ui.label(
                    RichText::new(nmp_core::display::short_npub(pubkey))
                        .small()
                        .weak(),
                );
                if !profile.nip05.is_empty() {
                    ui.label(
                        RichText::new(&profile.nip05)
                            .small()
                            .color(Color32::from_rgb(96, 165, 250)),
                    );
                }
            });
        });
        ui.add_space(4.0);

        // Follow / Unfollow — dispatches the existing nmp.follow / nmp.unfollow
        // actions through the bridge (mirrors the TUI's `f`/`F` handlers).
        ui.horizontal(|ui| {
            if ui.button("Follow").clicked() {
                let _ = self.bridge.follow(&pk);
            }
            if ui.button("Unfollow").clicked() {
                let _ = self.bridge.unfollow(&pk);
            }
        });
        ui.add_space(4.0);

        ui.separator();
        ui.add_space(4.0);

        let Some(author_feed) = feed else {
            ui.label("Loading posts…");
            return;
        };

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for entry in &author_feed.cards {
                    feed_card(
                        ui,
                        &entry.card,
                        &profiles,
                        &snap.embeds,
                        &mut self.tab,
                        &self.bridge,
                        &mut self.reply_to,
                    );
                    ui.add_space(4.0);
                }
            });
    }

    fn outbox_panel(&mut self, ui: &mut Ui, snap: &Snapshot) {
        ui.heading("Publish Outbox");
        ui.separator();

        let action_stages: Vec<ActionStageRow> =
            snap.projection("action_stages").unwrap_or_default();

        if action_stages.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label(RichText::new("No pending publishes").size(15.0).weak());
            });
            return;
        }

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("outbox_grid")
                    .num_columns(4)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(RichText::new("ID").small().strong());
                        ui.label(RichText::new("Status").small().strong());
                        ui.label(RichText::new("Reason").small().strong());
                        ui.label(RichText::new("Actions").small().strong());
                        ui.end_row();

                        for row in &action_stages {
                            // Truncated correlation ID
                            let short_id = if row.correlation_id.len() > 16 {
                                format!("{}…", &row.correlation_id[..13])
                            } else {
                                row.correlation_id.clone()
                            };
                            ui.label(RichText::new(short_id).monospace().small())
                                .on_hover_text(&row.correlation_id);

                            // Status
                            let is_terminal =
                                matches!(row.stage.as_str(), "published" | "failed" | "error");
                            let status_color = match row.stage.as_str() {
                                "publishing" => Color32::from_rgb(249, 115, 22),
                                "published" => Color32::from_rgb(74, 222, 128),
                                "failed" | "error" => Color32::from_rgb(248, 113, 113),
                                _ => Color32::from_rgb(148, 163, 184),
                            };
                            ui.label(RichText::new(&row.stage).color(status_color).small());

                            // Reason (if present)
                            if let Some(reason) = &row.reason {
                                ui.label(RichText::new(reason).small().weak());
                            } else {
                                ui.label(RichText::new("—").small().weak());
                            }

                            // Action buttons
                            ui.horizontal(|ui| {
                                if ui.small_button("Retry").clicked() {
                                    self.bridge.retry_publish(&row.correlation_id);
                                }
                                if ui.small_button("Cancel").clicked() {
                                    self.bridge.cancel_publish(&row.correlation_id);
                                }
                            });

                            ui.end_row();

                            // Ack terminal stages after they have been shown
                            // once so the kernel evicts them from action_stages
                            // and the outbox sidecar stops accumulating entries.
                            if is_terminal {
                                self.bridge.ack_action_stage(&row.correlation_id);
                            }
                        }
                    });
            });
    }

    pub(crate) fn status_color(connection: &str) -> (char, Color32) {
        let lower = connection.to_ascii_lowercase();
        if lower.contains("connected") || lower == "ready" || lower == "open" {
            ('●', Color32::from_rgb(74, 222, 128))
        } else if lower.contains("disconnected")
            || lower.contains("down")
            || lower.contains("failed")
        {
            ('○', Color32::from_rgb(248, 113, 113))
        } else {
            ('◌', Color32::from_rgb(249, 115, 22))
        }
    }
}

// ---------------------------------------------------------------------------
// Note card widget
// ---------------------------------------------------------------------------

/// Resolve a hex pubkey to a display label via the kernel's pre-merged
/// `resolved_profiles` map, falling back to a truncated npub when no kind:0
/// display name has arrived (aim.md §2 — presentation owns the fallback).
fn display_label(pubkey: &str, profiles: &HashMap<String, ProfileCard>) -> String {
    profiles
        .get(pubkey)
        .and_then(|p| p.display_name.as_deref())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| nmp_core::display::short_npub(pubkey))
}

fn note_record_from_card(card: &TimelineEventCard) -> NoteRecord {
    NoteRecord {
        event_id: card.id.clone(),
        author: card.author_pubkey.clone(),
        created_at: card.created_at,
        content: card.content.clone(),
        refs: Nip10Refs::default(),
    }
}

fn thread_reply_target(snap: &Snapshot, event_id: &str) -> Option<NoteRecord> {
    let key = format!("nmp.feed.thread.{event_id}");
    let feed: ModularTimelineSnapshot = snap.projection(&key)?;
    let card = feed
        .cards
        .iter()
        .find(|entry| entry.card.id == event_id)
        .or_else(|| feed.cards.first())?;
    Some(note_record_from_card(&card.card))
}

/// Render one home-feed root card (`nmp.feed.home` → `TimelineEventCard`).
///
/// Display name is resolved from the snapshot's `resolved_profiles` map; the
/// card itself carries only raw protocol data (hex pubkey, Unix `created_at`,
/// verbatim `content`).
fn feed_card(
    ui: &mut Ui,
    card: &TimelineEventCard,
    profiles: &HashMap<String, ProfileCard>,
    embeds: &HashMap<String, nmp_content::EmbeddedEventEnvelope>,
    tab: &mut AppTab,
    bridge: &AppRuntime,
    reply_to: &mut Option<NoteRecord>,
) {
    let author_display = display_label(&card.author_pubkey, profiles);
    let initials =
        nmp_core::display::avatar_initials(&nmp_core::display::to_npub(&card.author_pubkey));
    let color = nmp_core::display::avatar_color_hex(&card.author_pubkey);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let created_at_display = nmp_core::display::format_ago_secs(now, card.created_at);

    Frame::group(ui.style())
        .fill(ui.visuals().faint_bg_color)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                avatar(ui, &initials, &color);
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    // Repost attribution line: "↻ reposted by <name>".
                    if let Some(repost) = &card.reposted_by {
                        let reposter = display_label(&repost.author_pubkey, profiles);
                        ui.label(
                            RichText::new(format!("↻ reposted by {reposter}"))
                                .small()
                                .weak()
                                .color(Color32::from_rgb(148, 163, 184)),
                        );
                    }
                    ui.horizontal(|ui| {
                        // Clickable author name → open author view.
                        if ui.button(RichText::new(&author_display).strong()).clicked() {
                            *tab = AppTab::Author(card.author_pubkey.clone());
                            bridge.open_author(&card.author_pubkey);
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(&created_at_display).weak().small());
                        });
                    });
                    // The kernel projection already unwraps kind:6 reposts
                    // before the card reaches the shell — `content` is never
                    // raw kind:6 JSON here.  Use the typed `reposted_by` field
                    // for the repost badge instead of JSON-parsing `content`.
                    if card.reposted_by.is_some() {
                        ui.label(
                            RichText::new("↩ repost")
                                .small()
                                .weak()
                                .color(Color32::from_rgb(148, 163, 184)),
                        );
                    }
                    let text = &card.content;
                    if !text.is_empty() {
                        // Render rich body; treat any click anywhere on it as
                        // "open thread".  note_body renders an
                        // `horizontal_wrapped` group of inline widgets — we
                        // capture the response of the whole group by wrapping in
                        // a `ui.scope` and checking `response.response.clicked()`.
                        let scope = ui.scope(|ui| {
                            note_body(ui, text.as_ref(), embeds);
                        });
                        if scope.response.interact(egui::Sense::click()).clicked() {
                            *tab = AppTab::Thread(card.id.clone());
                            bridge.open_thread(&card.id);
                        }
                    }
                    // Like / Repost / Zap row.
                    ui.horizontal(|ui| {
                        if ui.small_button("↩ Reply").clicked() {
                            *reply_to = Some(note_record_from_card(card));
                        }
                        if ui.small_button("❤ Like").clicked() {
                            let _ = bridge.react(&card.id, "+");
                        }
                        if ui.small_button("🔁 Repost").clicked() {
                            let _ = bridge.repost(&card.id, &card.author_pubkey);
                        }
                        if ui.small_button("⚡ Zap").clicked() {
                            // Default amount: 21 sats = 21,000 msats.
                            let _ = bridge.zap(&card.author_pubkey, 21_000, &card.id);
                        }
                    });
                });
            });
        });
}

#[cfg(test)]
mod reply_tests {
    use super::*;

    #[test]
    fn reply_record_from_card_carries_raw_parent_fields() {
        let card = TimelineEventCard {
            id: "event-id".to_string(),
            author_pubkey: "author-pubkey".to_string(),
            created_at: 42,
            content: "parent".to_string(),
            ..Default::default()
        };

        let record = note_record_from_card(&card);

        assert_eq!(record.event_id, "event-id");
        assert_eq!(record.author, "author-pubkey");
        assert_eq!(record.created_at, 42);
        assert_eq!(record.content, "parent");
        assert_eq!(record.refs, Nip10Refs::default());
    }
}

pub(crate) fn relay_role_label(role: &str) -> &str {
    match role {
        "both" => "Both",
        "read" => "Read",
        "write" => "Write",
        "indexer" => "Index",
        "both,indexer" => "Both + Index",
        "read,indexer" => "Read + Index",
        "write,indexer" => "Write + Index",
        other if other.is_empty() => "Both",
        other => other,
    }
}

fn avatar(ui: &mut Ui, initials: &str, color_hex: &str) {
    let size = egui::vec2(36.0, 36.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter();
    painter.circle_filled(rect.center(), 18.0, hex_color(color_hex));
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        initials,
        egui::FontId::proportional(14.0),
        Color32::WHITE,
    );
}

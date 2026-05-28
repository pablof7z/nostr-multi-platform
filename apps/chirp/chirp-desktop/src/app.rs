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

use crate::bridge::AppRuntime;
use crate::render::{effective_content, hex_color, note_body};
use crate::snapshot::{
    AuthorViewPayload, RelayEditRow, Snapshot, ThreadViewPayload,
    TimelineItem,
};

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum AppTab {
    Home,
    Thread(String),
    Author(String),
    Settings,
}

pub struct DesktopApp {
    bridge: AppRuntime,
    latest: Arc<Mutex<Option<Snapshot>>>,
    tab: AppTab,
    compose: String,
    nsec_input: String,
    new_relay_url: String,
    new_relay_role: String,
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
                let env = match nmp_core::decode_update_frame(&event.payload) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                let nmp_core::UpdateEnvelope::Snapshot(v) = env else {
                    continue;
                };
                let snap: Snapshot = match serde_json::from_value(v) {
                    Ok(s) => s,
                    Err(_) => continue,
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
            nsec_input: String::new(),
            new_relay_url: String::new(),
            new_relay_role: "both".to_string(),
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

        if matches!(self.tab, AppTab::Home) {
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
                    ui.label(
                        RichText::new(format!("{} {}", r.role, r.connection)).color(color),
                    )
                    .on_hover_text(&r.relay_url);
                    ui.separator();
                }
                ui.label(format!(
                    "{} notes · {} rx · {} visible",
                    snap.metrics.note_events,
                    snap.metrics.events_rx,
                    snap.metrics.visible_items
                ));
            });
            ui.add_space(4.0);
        });
    }

    fn sidebar(&mut self, ctx: &egui::Context, snap: &Snapshot) {
        SidePanel::left("sidebar").resizable(false).width_range(140.0..=180.0).show(ctx, |ui| {
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
                .selectable_label(
                    matches!(
                        current_tab,
                        AppTab::Author(_)
                    ),
                    "👤  Profile",
                )
                .clicked()
            {
                if let Some(ref pk) = snap.active_account {
                    self.tab = AppTab::Author(pk.clone());
                    self.bridge.open_author(pk);
                }
            }
            if ui
                .selectable_label(matches!(current_tab, AppTab::Settings), "⚙️  Settings")
                .clicked()
            {
                self.tab = AppTab::Settings;
            }

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);

            // Active account mini-card
            if let Some(ref pk) = snap.active_account {
                let name = snap
                    .profile
                    .display_name
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| &snap.profile.npub);
                ui.label(RichText::new(name).strong().small());
                ui.label(
                    RichText::new(nmp_core::display::short_npub(pk)).small().weak(),
                );
            } else {
                ui.label(RichText::new("No account").small().weak());
            }
        });
    }

    fn content(
        &mut self,
        ctx: &egui::Context,
        snap: &Snapshot,
    ) {
        let tab = self.tab.clone();
        CentralPanel::default().show(ctx, |ui| {
            match tab {
                AppTab::Home => self.timeline(ui, snap),
                AppTab::Thread(ref event_id) => {
                    let payload: Option<ThreadViewPayload> = snap.projection("thread_view");
                    self.thread_view(ui, snap, event_id, payload);
                }
                AppTab::Author(ref pubkey) => {
                    let payload: Option<AuthorViewPayload> = snap.projection("author_view");
                    self.author_view(ui, snap, pubkey, payload);
                }
                AppTab::Settings => self.settings_view(ui, snap),
            }
        });
    }

    fn compose_bar(&mut self, ctx: &egui::Context, snap: &Snapshot) {
        TopBottomPanel::bottom("compose").show(ctx, |ui| {
            ui.add_space(6.0);

            let signed_in = snap.active_account.is_some();

            if let Some(err) = &snap.last_error_toast {
                ui.colored_label(Color32::from_rgb(248, 113, 113), err);
            }

            ui.horizontal(|ui| {
                let hint = if signed_in {
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
                    if ui.add_enabled(can_send, egui::Button::new("Publish")).clicked() {
                        let _ = self.bridge.publish_note(self.compose.trim(), None);
                        self.compose.clear();
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
    fn timeline(&mut self, ui: &mut Ui, snap: &Snapshot,
    ) {
        if snap.items.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label(
                    RichText::new("Connecting to relays…")
                        .size(15.0)
                        .weak(),
                );
                ui.label(
                    RichText::new("Live timeline will appear here.").weak(),
                );
            });
            return;
        }

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for item in &snap.items {
                    note_card(ui, item, &mut self.tab, &self.bridge);
                    ui.add_space(6.0);
                }
            });
    }

    fn thread_view(
        &mut self,
        ui: &mut Ui,
        _snap: &Snapshot,
        _event_id: &str,
        payload: Option<ThreadViewPayload>,
    ) {
        ui.horizontal(|ui| {
            if ui.button("← Back").clicked() {
                self.tab = AppTab::Home;
                self.bridge.close_thread();
            }
            ui.label(RichText::new("Thread").strong());
        });
        ui.separator();

        let Some(thread) = payload else {
            ui.label("Loading thread…");
            return;
        };

        ui.label(
            RichText::new(format!("Root: {}", &thread.root_event_id[..16]))
                .small()
                .weak(),
        );
        ui.add_space(4.0);

        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for item in &thread.items {
                note_card(ui, item, &mut self.tab, &self.bridge);
                ui.add_space(4.0);
            }
        });
    }

    fn author_view(
        &mut self,
        ui: &mut Ui,
        _snap: &Snapshot,
        pubkey: &str,
        payload: Option<AuthorViewPayload>,
    ) {
        ui.horizontal(|ui| {
            if ui.button("← Back").clicked() {
                self.tab = AppTab::Home;
                self.bridge.close_author();
            }
            ui.label(RichText::new("Profile").strong());
        });
        ui.separator();

        let Some(author) = payload else {
            ui.label("Loading profile…");
            return;
        };

        // Profile header
        let initials = nmp_core::display::avatar_initials(
            &nmp_core::display::to_npub(pubkey),
        );
        let color = nmp_core::display::avatar_color_hex(pubkey);
        ui.horizontal(|ui| {
            avatar(ui, &initials, &color);
            ui.add_space(8.0);
            ui.vertical(|ui| {
                let name = author
                    .profile
                    .display_name
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("(no name)");
                ui.label(RichText::new(name).size(16.0).strong());
                ui.label(
                    RichText::new(nmp_core::display::short_npub(pubkey)).small().weak(),
                );
                if !author.profile.nip05.is_empty() {
                    ui.label(RichText::new(&author.profile.nip05).small().color(Color32::from_rgb(96, 165, 250)));
                }
            });
        });
        ui.add_space(4.0);

        // Follow / unfollow button
        if let Some(action) = &author.primary_action {
            if let Some(dispatch) = &action.dispatch {
                let label = &action.label;
                if ui.button(label).clicked() {
                    let _ = self.bridge.dispatch_action(&dispatch.namespace, &dispatch.body_json);
                }
            }
        }

        ui.separator();
        ui.label(RichText::new(format!("{} notes", author.note_count_display)).strong());
        ui.add_space(4.0);

        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for item in &author.items {
                note_card(ui, item, &mut self.tab, &self.bridge);
                ui.add_space(4.0);
            }
        });
    }

    fn settings_view(&mut self,
        ui: &mut Ui,
        snap: &Snapshot,
    ) {
        ui.heading("Settings");
        ui.separator();

        // Account section
        ui.label(RichText::new("Account").strong());
        if let Some(ref pk) = snap.active_account {
            let name = snap
                .profile
                .display_name
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("(no name)");
            ui.label(format!("Name: {name}"));
            ui.label(format!("Pubkey: {}", nmp_core::display::short_npub(pk)));
        } else {
            ui.label("No active account.");
            ui.horizontal(|ui| {
                if ui.button("Create new account").clicked() {
                    self.bridge.create_account(
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
                    self.bridge.open_timeline();
                }
            });
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut self.nsec_input)
                        .hint_text("nsec1… or hex secret")
                        .desired_width(260.0)
                        .password(true),
                );
                if ui.button("Sign in").clicked() && !self.nsec_input.trim().is_empty() {
                    self.bridge.sign_in_nsec(self.nsec_input.trim().to_string());
                    self.nsec_input.clear();
                    self.bridge.open_timeline();
                }
            });
        }

        ui.add_space(12.0);
        ui.separator();

        // Relays section
        ui.label(RichText::new("Relays").strong());
        let rows: Vec<RelayEditRow> = snap.projection("relay_edit_rows").unwrap_or_default();
        egui::Grid::new("relay_grid")
            .num_columns(3)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                ui.label(RichText::new("URL").small().strong());
                ui.label(RichText::new("Role").small().strong());
                ui.label(RichText::new("Status").small().strong());
                ui.end_row();
                for r in &rows {
                    ui.label(&r.url);
                    ui.label(RichText::new(&r.role_label).color(hex_color(&r.role_tint)));
                    let status = snap
                        .relay_statuses
                        .iter()
                        .find(|s| s.relay_url == r.url)
                        .map(|s| s.connection.clone())
                        .unwrap_or_else(|| "unknown".to_string());
                    ui.label(RichText::new(status).small());
                    ui.end_row();
                }
            });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add(
                TextEdit::singleline(&mut self.new_relay_url)
                    .hint_text("wss://relay.example.com")
                    .desired_width(220.0),
            );
            egui::ComboBox::from_id_source("relay_role")
                .selected_text(&self.new_relay_role)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.new_relay_role, "both".to_string(), "both");
                    ui.selectable_value(&mut self.new_relay_role, "read".to_string(), "read");
                    ui.selectable_value(
                        &mut self.new_relay_role,
                        "write".to_string(),
                        "write",
                    );
                    ui.selectable_value(
                        &mut self.new_relay_role,
                        "indexer".to_string(),
                        "indexer",
                    );
                });
            if ui.button("Add relay").clicked() && !self.new_relay_url.trim().is_empty() {
                self.bridge
                    .add_relay(self.new_relay_url.trim(), &self.new_relay_role);
                self.new_relay_url.clear();
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Note card widget
// ---------------------------------------------------------------------------

fn note_card(
    ui: &mut Ui,
    item: &TimelineItem,
    tab: &mut AppTab,
    bridge: &AppRuntime,
) {
    let author_display = nmp_core::display::short_npub(&item.author_pubkey);
    let initials = nmp_core::display::avatar_initials(
        &nmp_core::display::to_npub(&item.author_pubkey),
    );
    let color = nmp_core::display::avatar_color_hex(&item.author_pubkey);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let created_at_display = nmp_core::display::format_ago_secs(now, item.created_at);

    Frame::group(ui.style())
        .fill(ui.visuals().faint_bg_color)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                avatar(ui, &initials, &color);
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        // Clickable author name
                        if ui
                            .button(RichText::new(&author_display).strong())
                            .clicked()
                        {
                            *tab = AppTab::Author(item.author_pubkey.clone());
                            bridge.open_author(&item.author_pubkey);
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(&created_at_display).weak().small());
                            if item.relay_count > 1 {
                                ui.label(
                                    RichText::new(format!("·{}×", item.relay_count))
                                        .weak()
                                        .small(),
                                );
                            }
                        });
                    });
                    let (text, is_repost) = effective_content(&item.content);
                    if is_repost {
                        ui.label(
                            RichText::new("↩ repost")
                                .small()
                                .weak()
                                .color(Color32::from_rgb(148, 163, 184)),
                        );
                    }
                    if !text.is_empty() {
                        // Clickable body → open thread
                        let response = ui.label(text.as_ref());
                        if response.clicked() {
                            let target = if item.nav_target_id.is_empty() {
                                item.id.clone()
                            } else {
                                item.nav_target_id.clone()
                            };
                            *tab = AppTab::Thread(target.clone());
                            bridge.open_thread(&target);
                        }
                        note_body(ui, text.as_ref());
                    }
                    // Like button row
                    ui.horizontal(|ui| {
                        if ui.small_button("❤ Like").clicked() {
                            let target = if item.nav_target_id.is_empty() {
                                &item.id
                            } else {
                                &item.nav_target_id
                            };
                            let _ = bridge.react(target, "+");
                        }
                    });
                });
            });
        });
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

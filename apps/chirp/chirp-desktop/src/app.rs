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
    TimelineItem, ActionStageRow,
};
use nmp_chirp_config;

// ---------------------------------------------------------------------------
// Helper functions — typed OP-feed decode (mirrors chirp-tui approach)
// ---------------------------------------------------------------------------

/// Extract the typed OP-feed `nmp.feed.home` sidecar and re-serialize it as a
/// generic `Value` for insertion into the snapshot projections map.
///
/// Returns `None` when the projection is absent, the schema id does not match
/// [`nmp_nip01::OP_FEED_SCHEMA_ID`], or the FlatBuffers payload is corrupt.
/// Both of these cases fall back to the generic `Value` projection that the
/// snapshot already carries.
fn extract_home_feed_from_typed(
    projections: &[nmp_core::TypedProjectionData],
) -> Option<serde_json::Value> {
    let proj = projections
        .iter()
        .find(|p| p.key == "nmp.feed.home" && p.schema_id == nmp_nip01::OP_FEED_SCHEMA_ID)?;
    nmp_nip01::decode_op_feed_snapshot(&proj.payload)
        .ok()
        .and_then(|snapshot| serde_json::to_value(&snapshot).ok())
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum AppTab {
    Home,
    Thread(String),
    Author(String),
    Settings,
    Diagnostics,
    Outbox,
}

pub struct DesktopApp {
    bridge: AppRuntime,
    latest: Arc<Mutex<Option<Snapshot>>>,
    tab: AppTab,
    compose: String,
    nsec_input: String,
    bunker_relay_input: String,
    bunker_uri: Option<String>,
    new_relay_url: String,
    new_relay_role: String,
    edit_display_name: String,
    edit_about: String,
    edit_picture: String,
    show_edit_profile: bool,
    nwc_input: String,
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
                let Ok((value, typed)) =
                    nmp_core::decode_snapshot_with_typed(&event.payload)
                else {
                    continue;
                };
                let mut snap: Snapshot = match serde_json::from_value(value) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                // Prefer the typed OP-feed sidecar when present (same as TUI).
                if let Some(feed) = extract_home_feed_from_typed(&typed) {
                    snap.projections.insert("nmp.feed.home".to_string(), feed);
                }
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
            bunker_relay_input: "wss://relay.primal.net".to_string(),
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
            if ui
                .selectable_label(matches!(current_tab, AppTab::Diagnostics), "📊  Diagnostics")
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
                AppTab::Dms => {
                    ui.heading("Direct Messages");
                    ui.separator();
                    let payload: Option<crate::snapshot::DmConversationSnapshot> =
                        snap.projection("nmp.nip17.dm_inbox");
                    match payload {
                        None => {
                            ui.vertical_centered(|ui| {
                                ui.add_space(40.0);
                                ui.label(RichText::new("No direct messages").weak());
                            });
                        }
                        Some(dm_snapshot) => {
                            if dm_snapshot.conversations.is_empty() {
                                ui.vertical_centered(|ui| {
                                    ui.add_space(40.0);
                                    ui.label(RichText::new("No conversations").weak());
                                });
                            } else {
                                ScrollArea::vertical()
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        for conversation in &dm_snapshot.conversations {
                                            Frame::group(ui.style())
                                                .fill(ui.visuals().faint_bg_color)
                                                .show(ui, |ui| {
                                                    let peer_name = if conversation.peer_display.is_empty() {
                                                        nmp_core::display::short_npub(&conversation.peer_pubkey)
                                                    } else {
                                                        conversation.peer_display.clone()
                                                    };
                                                    ui.label(RichText::new(&peer_name).strong());
                                                    if let Some(last_msg) = conversation.messages.last() {
                                                        let preview = if last_msg.content.len() > 60 {
                                                            format!("{}…", &last_msg.content[..57])
                                                        } else {
                                                            last_msg.content.clone()
                                                        };
                                                        ui.label(RichText::new(&preview).small().weak());
                                                    }
                                                });
                                            ui.add_space(6.0);
                                        }
                                    });
                            }
                        }
                    }
                }
                AppTab::Settings => self.settings_view(ui, snap),
                AppTab::Diagnostics => self.diagnostics_panel(ui, snap),
                AppTab::Outbox => self.outbox_panel(ui, snap),
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
                        nmp_chirp_config::chirp_default_relay_bootstrap()
                            .iter()
                            .map(|e| (e.url.to_string(), e.role.to_string()))
                            .collect(),
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


        // Bunker login section
        if snap.active_account.is_none() {
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut self.bunker_relay_input)
                        .hint_text("wss://relay.example.com")
                        .desired_width(260.0),
                );
                if ui.button("Connect with bunker").clicked() {
                    match self.bridge.connect_bunker(self.bunker_relay_input.trim()) {
                        Ok(uri) => self.bunker_uri = Some(uri),
                        Err(e) => eprintln!("bunker connect error: {e}"),
                    }
                }
            });
            if let Some(ref uri) = self.bunker_uri {
                ui.label(RichText::new("Scan or paste nostrconnect:// URI:").small());
                ui.text_edit_singleline(&mut uri.clone());
                if ui.button("Cancel").clicked() {
                    self.bridge.cancel_bunker_handshake();
                    self.bunker_uri = None;
                }
            }
        }

        // Edit profile section
        if let Some(ref _pk) = snap.active_account {
            ui.add_space(12.0);
            ui.separator();
            if !self.show_edit_profile {
                if ui.button("Edit Profile").clicked() {
                    self.show_edit_profile = true;
                    // Populate fields from current profile
                    self.edit_display_name = snap
                        .profile
                        .display_name
                        .as_deref()
                        .unwrap_or("")
                        .to_string();
                    self.edit_about = snap.profile.about.clone();
                    self.edit_picture = snap
                        .profile
                        .picture_url
                        .as_deref()
                        .unwrap_or("")
                        .to_string();
                }
            } else {
                ui.label(RichText::new("Edit Profile").strong());
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    ui.text_edit_singleline(&mut self.edit_display_name);
                });
                ui.horizontal(|ui| {
                    ui.label("About:");
                    ui.text_edit_multiline(&mut self.edit_about);
                });
                ui.horizontal(|ui| {
                    ui.label("Picture URL:");
                    ui.text_edit_singleline(&mut self.edit_picture);
                });
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        let _ = self.bridge.publish_profile(
                            &self.edit_display_name,
                            &self.edit_about,
                            &self.edit_picture,
                        );
                        self.show_edit_profile = false;
                    }
                    if ui.button("Cancel").clicked() {
                        self.show_edit_profile = false;
                        self.edit_display_name.clear();
                        self.edit_about.clear();
                        self.edit_picture.clear();
                    }
                });
            }
        }

        ui.add_space(12.0);
        ui.separator();

        // Wallet section
        ui.label(RichText::new("Wallet (NIP-47)").strong());
        ui.horizontal(|ui| {
            ui.add(
                TextEdit::singleline(&mut self.nwc_input)
                    .hint_text("nostr+walletconnect://...")
                    .desired_width(340.0),
            );
            if ui.button("Connect").clicked() && !self.nwc_input.trim().is_empty() {
                match self.bridge.wallet_connect(self.nwc_input.trim()) {
                    Ok(_) => {
                        self.nwc_input.clear();
                    }
                    Err(e) => eprintln!("wallet connect error: {e}"),
                }
            }
        });
        if ui.button("Disconnect Wallet").clicked() {
            match self.bridge.wallet_disconnect() {
                Ok(_) => {},
                Err(e) => eprintln!("wallet disconnect error: {e}"),
            }
        }

        ui.add_space(12.0);
        ui.separator();

        // Relays section
        ui.label(RichText::new("Relays").strong());
        let rows: Vec<RelayEditRow> = snap.projection("relay_edit_rows").unwrap_or_default();
        egui::Grid::new("relay_grid")
            .num_columns(4)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                ui.label(RichText::new("URL").small().strong());
                ui.label(RichText::new("Role").small().strong());
                ui.label(RichText::new("Status").small().strong());
                ui.label(RichText::new("").small());
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
                    if ui.small_button("✕").clicked() {
                        self.bridge.remove_relay(&r.url);
                    }
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

    fn diagnostics_panel(&self, ui: &mut Ui, snap: &Snapshot) {
        ui.heading("Routing & Relay Diagnostics");
        ui.separator();

        // Relay summary
        let connected_count = snap
            .relay_statuses
            .iter()
            .filter(|r| {
                r.connection.eq_ignore_ascii_case("connected")
                    || r.connection.eq_ignore_ascii_case("ready")
            })
            .count();
        ui.label(RichText::new(format!(
            "Relays: {}/{} connected",
            connected_count,
            snap.relay_statuses.len()
        ))
        .strong());
        ui.add_space(8.0);

        // Relay list with status
        ui.label(RichText::new("Relay Status").strong().color(Color32::from_rgb(96, 165, 250)));
        ui.add_space(4.0);

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height(300.0)
            .show(ui, |ui| {
                egui::Grid::new("diagnostics_relays")
                    .num_columns(4)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.label(RichText::new("Relay").strong());
                        ui.label(RichText::new("Role").strong());
                        ui.label(RichText::new("Status").strong());
                        ui.label(RichText::new("Events").strong());
                        ui.end_row();

                        for relay in &snap.relay_statuses {
                            // Status dot
                            let (dot_char, dot_color) = Self::status_color(&relay.connection);
                            ui.label(RichText::new(dot_char).color(dot_color));

                            // URL (shortened)
                            let display_url = if relay.relay_url.len() > 30 {
                                format!("{}…", &relay.relay_url[..27])
                            } else {
                                relay.relay_url.clone()
                            };
                            ui.label(display_url).on_hover_text(&relay.relay_url);

                            // Role
                            let role_color = match relay.role.as_str() {
                                "read" => Color32::from_rgb(96, 165, 250),
                                "write" => Color32::from_rgb(34, 197, 94),
                                "indexer" => Color32::from_rgb(168, 85, 247),
                                _ => Color32::from_rgb(107, 114, 128),
                            };
                            ui.label(RichText::new(&relay.role).color(role_color));

                            // Status
                            let status_color = if relay.connection.eq_ignore_ascii_case("connected")
                                || relay.connection.eq_ignore_ascii_case("ready")
                            {
                                Color32::from_rgb(74, 222, 128)
                            } else if relay.connection.eq_ignore_ascii_case("disconnected")
                                || relay.connection.eq_ignore_ascii_case("down")
                            {
                                Color32::from_rgb(248, 113, 113)
                            } else {
                                Color32::from_rgb(249, 115, 22)
                            };
                            ui.label(
                                RichText::new(&relay.connection).color(status_color),
                            );

                            // Event count
                            ui.label(RichText::new(relay.events_rx.to_string()).weak().small());

                            ui.end_row();
                        }
                    });
            });

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(8.0);

        // Metrics summary
        ui.label(RichText::new("Snapshot Metrics").strong().color(Color32::from_rgb(96, 165, 250)));
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            ui.label(format!("Total events received: {}", snap.metrics.events_rx));
            ui.separator();
            ui.label(format!("Note events: {}", snap.metrics.note_events));
            ui.separator();
            ui.label(format!("Visible items: {}", snap.metrics.visible_items));
        });

        ui.add_space(8.0);
        ui.label(format!("Snapshot revision: {}", snap.rev));
    }

    fn outbox_panel(&mut self, ui: &mut Ui, snap: &Snapshot) {
        ui.heading("Publish Outbox");
        ui.separator();

        let action_stages: Vec<ActionStageRow> = snap.projection("action_stages").unwrap_or_default();

        if action_stages.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label(
                    RichText::new("No pending publishes")
                        .size(15.0)
                        .weak(),
                );
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
                        }
                    });
            });
    }

    fn status_color(connection: &str) -> (char, Color32) {
        let lower = connection.to_ascii_lowercase();
        if lower.contains("connected") || lower == "ready" || lower == "open" {
            ('●', Color32::from_rgb(74, 222, 128))
        } else if lower.contains("disconnected") || lower.contains("down") || lower.contains("failed") {
            ('○', Color32::from_rgb(248, 113, 113))
        } else {
            ('◌', Color32::from_rgb(249, 115, 22))
        }
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
                        if ui.small_button("⚡ Zap").clicked() {
                            let target = if item.nav_target_id.is_empty() {
                                &item.id
                            } else {
                                &item.nav_target_id
                            };
                            // Default amount: 21 sats = 21,000 msats
                            let _ = bridge.zap(&item.author_pubkey, 21_000, target);
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

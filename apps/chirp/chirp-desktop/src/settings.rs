//! Settings pane and diagnostics panel rendering.
//!
//! Split out of `app.rs` to stay within the 500-LOC file-size ceiling
//! (AGENTS.md). `settings_view` and `diagnostics_panel` are `DesktopApp`
//! methods registered here via a second `impl` block; Rust allows multiple
//! `impl` blocks across files.

use egui::{Color32, RichText, ScrollArea, TextEdit, Ui};
use zeroize::Zeroize;

use crate::app::{relay_role_label, DesktopApp};
use crate::snapshot::{AppRelay, BunkerHandshakeStatus, SignerStatus, Snapshot};

impl DesktopApp {
    pub(crate) fn settings_view(&mut self, ui: &mut Ui, snap: &Snapshot) {
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
                    self.bridge.sign_in_nsec(self.nsec_input.trim());
                    self.nsec_input.zeroize();
                    self.bridge.open_timeline();
                }
            });
        }

        if !snap.accounts.is_empty() {
            ui.add_space(8.0);
            egui::Grid::new("account_grid")
                .num_columns(4)
                .spacing([12.0, 4.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("Name").small().strong());
                    ui.label(RichText::new("Pubkey").small().strong());
                    ui.label(RichText::new("Status").small().strong());
                    ui.label(RichText::new("").small());
                    ui.end_row();

                    for account in &snap.accounts {
                        let name = account
                            .display_name
                            .as_deref()
                            .filter(|s| !s.is_empty())
                            .unwrap_or("(no name)");
                        ui.label(name);
                        ui.label(
                            RichText::new(nmp_core::display::short_npub(&account.pubkey))
                                .small()
                                .weak(),
                        )
                        .on_hover_text(&account.pubkey);
                        ui.label(if account.is_active { "Active" } else { "" });

                        let confirm_removal = self.pending_account_removal.as_deref()
                            == Some(account.pubkey.as_str());
                        ui.horizontal(|ui| {
                            if !account.is_active && ui.small_button("Switch").clicked() {
                                self.bridge.switch_account(&account.pubkey);
                                self.bridge.open_timeline();
                            }

                            if confirm_removal {
                                if ui.small_button("Confirm Remove").clicked() {
                                    self.bridge.remove_account(&account.pubkey);
                                    self.pending_account_removal = None;
                                }
                                if ui.small_button("Cancel").clicked() {
                                    self.pending_account_removal = None;
                                }
                            } else if ui.small_button("Remove").clicked() {
                                self.pending_account_removal = Some(account.pubkey.clone());
                            }
                        });
                        ui.end_row();
                    }
                });
        }

        // Bunker login section — decode the typed bunker_handshake sidecar so
        // connect-QR progress/success/failure is reflected in real time.
        // D3: the relay is Rust-owned (selected from the kernel relay config);
        // the shell no longer presents a relay input field.
        if snap.active_account.is_none() {
            ui.horizontal(|ui| {
                if ui.button("Connect with bunker").clicked() {
                    match self.bridge.connect_bunker() {
                        Ok(uri) => self.bunker_uri = Some(uri),
                        Err(e) => eprintln!("bunker connect error: {e}"),
                    }
                }
            });
            if let Some(ref uri) = self.bunker_uri {
                ui.label(RichText::new("Scan or paste nostrconnect:// URI:").small());
                ui.text_edit_singleline(&mut uri.clone());

                // Live handshake progress from the kernel's typed sidecar.
                let handshake: Option<BunkerHandshakeStatus> = snap.projection("bunker_handshake");
                if let Some(ref hs) = handshake {
                    // #1493 P9: the English stage label is derived in the shell
                    // from the raw `stage` token (Rust no longer pre-computes it).
                    let in_flight_label = desktop_stage_label(&hs.stage);
                    let (label, color) = if hs.is_terminal_success {
                        ("Connected!", Color32::from_rgb(74, 222, 128))
                    } else if hs.is_failed {
                        ("Failed", Color32::from_rgb(248, 113, 113))
                    } else if hs.is_in_flight {
                        (in_flight_label.as_str(), Color32::from_rgb(249, 115, 22))
                    } else {
                        ("Waiting…", Color32::from_rgb(148, 163, 184))
                    };
                    ui.label(RichText::new(label).color(color).small());
                    if let Some(ref msg) = hs.message {
                        ui.label(RichText::new(msg).small().weak());
                    }
                    if hs.is_terminal_success {
                        self.bunker_uri = None;
                    }
                }

                if ui.button("Cancel").clicked() {
                    self.bridge.cancel_bunker_handshake();
                    self.bunker_uri = None;
                }
            }

            // Signer state — shown when a remote signer is active (NIP-46/NIP-55).
            let signer: Option<SignerStatus> = snap.projection("signer_state");
            if let Some(ref ss) = signer {
                // #1493 P9: shell derives the label + tone from the raw `state`
                // token (Rust no longer pre-computes status_label/status_tone).
                let (status_label, status_tone) = desktop_signer_label_and_tone(&ss.state);
                let tone_color = match status_tone {
                    "active" => Color32::from_rgb(74, 222, 128),
                    "warning" => Color32::from_rgb(251, 191, 36),
                    "error" => Color32::from_rgb(248, 113, 113),
                    _ => Color32::from_rgb(148, 163, 184),
                };
                ui.label(
                    RichText::new(format!("Signer: {status_label}"))
                        .color(tone_color)
                        .small(),
                );
                if let Some(ref reason) = ss.reason {
                    ui.label(RichText::new(reason).small().weak());
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
                Ok(_) => {}
                Err(e) => eprintln!("wallet disconnect error: {e}"),
            }
        }

        ui.add_space(12.0);
        ui.separator();

        // Relays section
        ui.label(RichText::new("Relays").strong());
        let rows: Vec<AppRelay> = snap.projection("configured_relays").unwrap_or_default();
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
                    ui.label(RichText::new(relay_role_label(&r.role)));
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
                    ui.selectable_value(&mut self.new_relay_role, "write".to_string(), "write");
                    ui.selectable_value(&mut self.new_relay_role, "indexer".to_string(), "indexer");
                });
            if ui.button("Add relay").clicked() && !self.new_relay_url.trim().is_empty() {
                self.bridge
                    .add_relay(self.new_relay_url.trim(), &self.new_relay_role);
                self.new_relay_url.clear();
            }
        });

        // Publish the configured relay set as a NIP-65 kind:10002 event via the
        // existing nmp.nip65.publish_relay_list action.
        ui.add_space(8.0);
        ui.add_enabled_ui(!rows.is_empty(), |ui| {
            if ui.button("Publish Relay List").clicked() {
                let relays: Vec<(&str, &str)> = rows
                    .iter()
                    .map(|r| (r.url.as_str(), r.role.as_str()))
                    .collect();
                if let Err(e) = self.bridge.publish_relay_list(&relays) {
                    eprintln!("publish relay list error: {e}");
                }
            }
        });
    }

    pub(crate) fn diagnostics_panel(&self, ui: &mut Ui, snap: &Snapshot) {
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
        ui.label(
            RichText::new(format!(
                "Relays: {}/{} connected",
                connected_count,
                snap.relay_statuses.len()
            ))
            .strong(),
        );
        ui.add_space(8.0);

        // Relay list with status
        ui.label(
            RichText::new("Relay Status")
                .strong()
                .color(Color32::from_rgb(96, 165, 250)),
        );
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
                            ui.label(RichText::new(&relay.connection).color(status_color));

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
        ui.label(
            RichText::new("Snapshot Metrics")
                .strong()
                .color(Color32::from_rgb(96, 165, 250)),
        );
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
}

// ── Shell-side label derivation (#1493 P9 — labels-to-shells) ────────────────
//
// The kernel emits raw `stage` / `state` tokens; the desktop shell renders the
// English here (mirrors the deleted Rust `stage_label_for` /
// `signer_state_label_and_tone`, and the iOS/Android shell derivations).

/// English label for a bunker-handshake `stage` token.
fn desktop_stage_label(stage: &str) -> String {
    match stage {
        "idle" => "Idle",
        "connecting" => "Connecting to bunker relays…",
        "awaiting_pubkey" => "Awaiting bunker approval…",
        "ready" => "Connected",
        "failed" => "Bunker handshake failed",
        other => other,
    }
    .to_string()
}

/// English label + semantic tone for a signer `state` token.
fn desktop_signer_label_and_tone(state: &str) -> (&'static str, &'static str) {
    match state {
        "ready" | "connected" => ("Connected", "active"),
        "reconnecting" => ("Reconnecting…", "warning"),
        "awaiting_approval" => ("Waiting for approval…", "warning"),
        "unavailable" => ("Signer unavailable", "error"),
        "failed" => ("Connection failed", "error"),
        _ => ("Unknown", "inactive"),
    }
}

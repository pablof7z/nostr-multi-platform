//! egui application shell.
//!
//! Pure projection of the latest kernel [`Snapshot`] (D7: the UI owns no state
//! beyond the snapshot + transient input buffers). All mutations go back into
//! the kernel as `ActorCommand`s via [`KernelBridge`].

use eframe::App;
use egui::{Align, CentralPanel, Color32, Frame, Layout, RichText, ScrollArea, TopBottomPanel};

use crate::bridge::KernelBridge;
use crate::render::{effective_content, hex_color, note_body};
use crate::snapshot::Snapshot;

pub struct DesktopApp {
    bridge: KernelBridge,
    compose: String,
    nsec: String,
}

impl DesktopApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            bridge: KernelBridge::start(cc.egui_ctx.clone()),
            compose: String::new(),
            nsec: String::new(),
        }
    }
}

impl App for DesktopApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let snap = self.bridge.snapshot().unwrap_or_default();

        self.status_bar(ctx, &snap);
        self.compose_bar(ctx, &snap);
        self.timeline(ctx, &snap);
    }
}

impl DesktopApp {
    fn status_bar(&self, ctx: &egui::Context, snap: &Snapshot) {
        TopBottomPanel::top("status").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading("NMP");
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

    fn compose_bar(&mut self, ctx: &egui::Context, snap: &Snapshot) {
        TopBottomPanel::bottom("compose").show(ctx, |ui| {
            ui.add_space(6.0);

            let signed_in = snap.active_account.is_some();
            if !signed_in {
                ui.horizontal(|ui| {
                    ui.label("Sign in to publish:");
                    if ui.button("Create new account").clicked() {
                        self.bridge.create_account(
                            [("name".to_string(), "New User".to_string())].into(),
                            vec![
                                ("wss://relay.primal.net".to_string(), "both,indexer".to_string()),
                                ("wss://purplepag.es".to_string(), "both,indexer".to_string()),
                            ],
                        );
                        self.bridge.open_timeline();
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut self.nsec)
                            .hint_text("nsec1… or hex secret")
                            .desired_width(220.0)
                            .password(true),
                    );
                    if ui.button("Sign in").clicked() && !self.nsec.trim().is_empty() {
                        self.bridge.sign_in_nsec(self.nsec.trim().to_string());
                        self.nsec.clear();
                        self.bridge.open_timeline();
                    }
                });
            }

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
                    egui::TextEdit::multiline(&mut self.compose)
                        .hint_text(hint)
                        .desired_rows(2)
                        .desired_width(f32::INFINITY),
                );
            });
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let can_send = signed_in && !self.compose.trim().is_empty();
                    if ui
                        .add_enabled(can_send, egui::Button::new("Publish"))
                        .clicked()
                    {
                        self.bridge.publish_note(self.compose.trim().to_string());
                        self.compose.clear();
                    }
                    if !snap.profile.display.is_empty() {
                        ui.label(
                            RichText::new(format!("as {}", snap.profile.display)).weak(),
                        );
                    }
                });
            });
            ui.add_space(6.0);
        });
    }

    fn timeline(&self, ctx: &egui::Context, snap: &Snapshot) {
        CentralPanel::default().show(ctx, |ui| {
            if snap.items.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label(
                        RichText::new("Connecting to wss://relay.primal.net…")
                            .size(15.0)
                            .weak(),
                    );
                    ui.label(
                        RichText::new("Live seed timeline will appear here.").weak(),
                    );
                });
                return;
            }

            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for item in &snap.items {
                        note_card(ui, item);
                        ui.add_space(6.0);
                    }
                });
        });
    }
}

fn note_card(ui: &mut egui::Ui, item: &crate::snapshot::TimelineItem) {
    Frame::group(ui.style())
        .fill(ui.visuals().faint_bg_color)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                avatar(ui, &item.author_avatar_initials, &item.author_avatar_color);
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&item.author_display).strong());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(&item.created_at_display).weak().small());
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
                        ui.label(RichText::new("↩ repost").small().weak()
                            .color(Color32::from_rgb(148, 163, 184)));
                    }
                    if !text.is_empty() {
                        note_body(ui, text.as_ref());
                    }
                });
            });
        });
}

fn avatar(ui: &mut egui::Ui, initials: &str, color_hex: &str) {
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

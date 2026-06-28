//! egui application shell for Chirp Desktop.
//!
//! Renders the latest kernel [`Snapshot`] with left-sidebar navigation,
//! a central content area (timeline, thread, author, or settings),
//! a top status bar, and a bottom compose bar.

use std::sync::{Arc, Mutex};

use eframe::App;
use egui::{
    Align, CentralPanel, Color32, Layout, RichText, ScrollArea, SidePanel, TextEdit,
    TopBottomPanel, Ui,
};

use std::collections::HashMap;

use crate::bridge::AppRuntime;
use crate::diagnostics_flag;
use crate::snapshot::{
    ActionStageRow, FollowListSnapshot, ModularTimelineSnapshot, ProfileCard, Snapshot,
};
use crate::timeline_panel::{avatar, feed_card, note_record_from_card};
use crate::zap_amount::ZapAmountState;
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
    pub(crate) bunker_uri: Option<String>,
    pub(crate) new_relay_url: String,
    pub(crate) new_relay_role: String,
    pub(crate) edit_display_name: String,
    pub(crate) edit_about: String,
    pub(crate) edit_picture: String,
    pub(crate) show_edit_profile: bool,
    pub(crate) nwc_input: String,
    pub(crate) pending_account_removal: Option<String>,
    pub(crate) zap_amount: ZapAmountState,
    /// ADR-0063 (#1671 Lane F) — feed/list-row authors currently resolved at
    /// `profile.ref` / `CacheOk`. Diffed each frame against the visible authors so
    /// a pubkey is resolved when it appears and released when the view changes
    /// (D5/D7: every rendered reference resolves; bounded by what is open).
    pub(crate) resolved_feed_authors: std::collections::HashSet<String>,
}

impl DesktopApp {
    #[must_use]
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (bridge, rx) = AppRuntime::new().expect("Failed to boot Chirp kernel");
        let latest: Arc<Mutex<Option<Snapshot>>> = Arc::new(Mutex::new(None));

        let reader_latest = Arc::clone(&latest);
        let egui_ctx = cc.egui_ctx.clone();
        std::thread::spawn(move || {
            // ADR-0063 (#1671 Lane F): the persistent host-side mirror of the
            // `refs.profile` row-delta projection. Lives for the reader thread's
            // lifetime — row-deltas merge into it across frames (it is NOT
            // rebuilt per frame). The ONLY app-side store of hydrated profile
            // facts (D4); the snapshot's `refs_profiles` is a per-frame
            // materialisation of this cache, not a second cache.
            let mut ref_profiles = nmp_core::refs::RefProfileStore::new();
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
                let Some(snap) = crate::snapshot_decode::decode_snapshot_typed(
                    &event.payload,
                    &mut ref_profiles,
                ) else {
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
            bunker_uri: None,
            new_relay_url: String::new(),
            new_relay_role: "both".to_string(),
            edit_display_name: String::new(),
            edit_about: String::new(),
            edit_picture: String::new(),
            show_edit_profile: false,
            nwc_input: String::new(),
            pending_account_removal: None,
            zap_amount: ZapAmountState::default(),
            resolved_feed_authors: std::collections::HashSet::new(),
        }
    }

    /// ADR-0063 (#1671 Lane F) — auto-resolve every profile reference rendered in
    /// the current view at `profile.ref` / `CacheOk` and release any reference
    /// that is no longer on screen. "Reference" is the full visible set: row
    /// authors + reposters AND every note-body mention the cards render (the
    /// mention set is extracted via the same tokenisation the renderer walks, see
    /// `collect_feed_authors`). Runs once per frame; the kernel dedupes per pubkey
    /// so the per-frame resolve is idempotent. This closes the D7 coverage hole: a
    /// feed/profile/thread author OR mention cannot render without a live ref
    /// resolving it.
    fn sync_feed_author_refs(&mut self, snap: &Snapshot) {
        let mut visible: std::collections::HashSet<String> = std::collections::HashSet::new();
        // Home is always live; the open author/thread feed (if any) adds its rows.
        if let Some(feed) = snap.projection::<ModularTimelineSnapshot>("nmp.feed.home") {
            collect_feed_authors(&feed, &mut visible);
        }
        match &self.tab {
            AppTab::Author(pubkey) => {
                visible.insert(pubkey.clone());
                if let Some(feed) =
                    snap.projection::<ModularTimelineSnapshot>(&format!("nmp.feed.author.{pubkey}"))
                {
                    collect_feed_authors(&feed, &mut visible);
                }
            }
            AppTab::Thread(event_id) => {
                if let Some(feed) = snap
                    .projection::<ModularTimelineSnapshot>(&format!("nmp.feed.thread.{event_id}"))
                {
                    collect_feed_authors(&feed, &mut visible);
                }
            }
            _ => {}
        }

        for pubkey in visible.difference(&self.resolved_feed_authors) {
            self.bridge.resolve_feed_author_ref(pubkey);
        }
        for pubkey in self.resolved_feed_authors.difference(&visible) {
            self.bridge.release_feed_author_ref(pubkey);
        }
        self.resolved_feed_authors = visible;
    }

    fn snapshot(&self) -> Option<Snapshot> {
        self.latest.lock().ok().and_then(|s| s.clone())
    }

    /// ADR-0063 (#1671 Lane F) — the ONE desktop tab/view-transition chokepoint.
    ///
    /// `open_author` resolves `profile.card` / `Live` and `open_thread` opens a
    /// thread feed; both are refcounted by (namespace, key, consumer_id) in the
    /// ABI, so leaving or replacing an Author/Thread view WITHOUT releasing the
    /// outgoing key leaks it Live forever. Every navigation that changes
    /// `self.tab` MUST go through here: it releases the outgoing Author/Thread
    /// ref BEFORE opening the next view, covering Author A→B (release A, open B),
    /// Author→Home/Settings/Dms (release A), Author→Thread / Thread→Author
    /// (release the outgoing, open the incoming), and Thread→Thread. Switching to
    /// the same tab is a no-op (no spurious release/re-open). Feed/list-row author
    /// + mention refs (`profile.ref` / `CacheOk`) are handled separately by the
    /// per-frame `sync_feed_author_refs` diff — this chokepoint owns only the
    /// open-view (`profile.card` / thread-feed) lifecycle.
    pub(crate) fn navigate_to(&mut self, next: AppTab) {
        let Some((close, open)) = view_transition(&self.tab, &next) else {
            return;
        };
        // Release the OUTGOING open-view ref first (no leak on replace/leave)…
        match close {
            ViewRef::Author(pk) => self.bridge.close_author(&pk),
            ViewRef::Thread(id) => self.bridge.close_thread(&id),
            ViewRef::None => {}
        }
        // …then open the INCOMING view's ref.
        match open {
            ViewRef::Author(pk) => self.bridge.open_author(&pk),
            ViewRef::Thread(id) => self.bridge.open_thread(&id),
            ViewRef::None => {
                if matches!(next, AppTab::Home) {
                    self.bridge.open_timeline();
                }
            }
        }
        self.tab = next;
    }
}

/// ADR-0063 (#1671 Lane F) — the open-view (`profile.card` / thread-feed)
/// reference a tab holds, if any. `Home`/`Settings`/`Dms`/`Diagnostics`/`Outbox`
/// hold none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ViewRef {
    None,
    Author(String),
    Thread(String),
}

/// ADR-0063 (#1671 Lane F) — pure decision for `navigate_to`: given the current
/// and next tab, what open-view ref must be released and what must be opened.
/// `None` means "same tab, do nothing". Extracted as a pure function so the
/// release-before-open contract is unit-testable without booting the FFI kernel.
fn view_transition(current: &AppTab, next: &AppTab) -> Option<(ViewRef, ViewRef)> {
    if current == next {
        return None;
    }
    Some((tab_view_ref(current), tab_view_ref(next)))
}

fn tab_view_ref(tab: &AppTab) -> ViewRef {
    match tab {
        AppTab::Author(pk) => ViewRef::Author(pk.clone()),
        AppTab::Thread(id) => ViewRef::Thread(id.clone()),
        _ => ViewRef::None,
    }
}

/// ADR-0063 (#1671 Lane F) — collect every profile pubkey rendered by `feed`
/// into `out`: root author + repost attribution AND every note-body mention the
/// card renders. The mention set is extracted with the SAME tokenisation the
/// renderer walks (`render::collect_body_mention_pubkeys` mirrors `note_body`),
/// so a mentioned pubkey that is not also an author/reposter still gets resolved
/// and its name renders instead of the fallback npub (D7 — every rendered
/// reference resolves). Raw hex only (ADR-0032).
fn collect_feed_authors(
    feed: &ModularTimelineSnapshot,
    out: &mut std::collections::HashSet<String>,
) {
    for entry in &feed.cards {
        if !entry.card.author_pubkey.is_empty() {
            out.insert(entry.card.author_pubkey.clone());
        }
        if let Some(repost) = &entry.card.reposted_by {
            if !repost.author_pubkey.is_empty() {
                out.insert(repost.author_pubkey.clone());
            }
        }
        // Rendered note-body mentions (render.rs note_body / mention_label).
        crate::render::collect_body_mention_pubkeys(&entry.card.content, out);
    }
}

// ---------------------------------------------------------------------------
// egui App trait
// ---------------------------------------------------------------------------

impl App for DesktopApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let snap = self.snapshot().unwrap_or_default();
        if matches!(self.tab, AppTab::Diagnostics) && !diagnostics_flag::enabled() {
            self.navigate_to(AppTab::Home);
        }

        // ADR-0063 (#1671 Lane F): resolve every visible feed/profile/thread
        // author at profile.ref/CacheOk (release those no longer on screen) so
        // names/avatars render through the unified resolve_ref path (D7).
        self.sync_feed_author_refs(&snap);

        self.status_bar(ctx, &snap);
        self.sidebar(ctx, &snap);
        self.content(ctx, &snap);

        if matches!(self.tab, AppTab::Home | AppTab::Thread(_)) || self.reply_to.is_some() {
            self.compose_bar(ctx, &snap);
        }

        self.zap_amount_window(ctx);
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

                // ADR-0063 (#1671 Lane F): every sidebar transition routes
                // through `navigate_to` so leaving an Author/Thread view
                // releases its open-view ref (no leak on tab switch).
                if ui
                    .selectable_label(matches!(current_tab, AppTab::Home), "🏠  Home")
                    .clicked()
                {
                    self.navigate_to(AppTab::Home);
                }
                if ui
                    .selectable_label(matches!(current_tab, AppTab::Author(_)), "👤  Profile")
                    .clicked()
                {
                    if let Some(ref pk) = snap.active_account {
                        self.navigate_to(AppTab::Author(pk.clone()));
                    }
                }
                if ui
                    .selectable_label(matches!(current_tab, AppTab::Dms), "💬  DMs")
                    .clicked()
                {
                    self.navigate_to(AppTab::Dms);
                }
                if ui
                    .selectable_label(matches!(current_tab, AppTab::Settings), "⚙️  Settings")
                    .clicked()
                {
                    self.navigate_to(AppTab::Settings);
                }
                if diagnostics_flag::enabled() {
                    if ui
                        .selectable_label(
                            matches!(current_tab, AppTab::Diagnostics),
                            "📊  Diagnostics",
                        )
                        .clicked()
                    {
                        self.navigate_to(AppTab::Diagnostics);
                    }
                }
                if ui
                    .selectable_label(matches!(current_tab, AppTab::Outbox), "📤  Outbox")
                    .clicked()
                {
                    self.navigate_to(AppTab::Outbox);
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
                // ADR-0063 (#1671 Lane F): read from the refs.profile mirror.
                let profiles: HashMap<String, ProfileCard> = snap.refs_profiles.clone();
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
                    if ui.add_enabled(can_send, egui::Button::new(label)).clicked() {
                        let _ = self.bridge.publish_note(self.compose.trim(), reply_target);
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

    fn zap_amount_window(&mut self, ctx: &egui::Context) {
        if let Some((target, amount_msats)) = self.zap_amount.show(ctx) {
            let _ = self.bridge.zap(
                &target.recipient_pubkey,
                amount_msats,
                &target.target_event_id,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

impl DesktopApp {
    fn thread_view(
        &mut self,
        ui: &mut Ui,
        snap: &Snapshot,
        event_id: &str,
        feed: Option<ModularTimelineSnapshot>,
    ) {
        let _eid = event_id.to_string();
        // ADR-0063 (#1671 Lane F): read from the refs.profile mirror.
        let profiles: HashMap<String, ProfileCard> = snap.refs_profiles.clone();
        ui.horizontal(|ui| {
            if ui.button("← Back").clicked() {
                // ADR-0063 (#1671 Lane F): release the outgoing thread feed via
                // the navigation chokepoint (no manual close_thread leak).
                self.navigate_to(AppTab::Home);
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

        let mut nav: Option<AppTab> = None;
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for entry in &thread_feed.cards {
                    if let Some(target) = feed_card(
                        ui,
                        &entry.card,
                        &profiles,
                        &snap.embeds,
                        &mut nav,
                        &mut self.reply_to,
                        &self.bridge,
                    ) {
                        self.zap_amount.open(target);
                    }
                    ui.add_space(4.0);
                }
            });
        // ADR-0063 (#1671 Lane F): route the in-card transition through the
        // chokepoint so this thread feed is released before the next view opens.
        if let Some(next) = nav {
            self.navigate_to(next);
        }
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
                // ADR-0063 (#1671 Lane F): release the outgoing author's
                // profile.card ref via the navigation chokepoint (no leak).
                self.navigate_to(AppTab::Home);
            }
            ui.label(RichText::new("Profile").strong());
        });
        ui.separator();

        // V-112 (ADR-0042): author_view projection deleted; items come from flat feed.
        // Profile header reads the refs_profiles mirror (ADR-0063 #1671 Lane F:
        // populated by resolve_ref at profile.card/Live, kept live by open_author).
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

        let follow_list: FollowListSnapshot =
            snap.projection("nmp.follow_list").unwrap_or_default();
        let following = follow_list
            .follows
            .iter()
            .any(|entry| entry.pubkey == pubkey);
        let is_self = snap.active_account.as_deref() == Some(pubkey);
        if !is_self {
            ui.horizontal(|ui| {
                if following {
                    if ui.button("Following").clicked() {
                        let _ = self.bridge.unfollow(&pk);
                    }
                } else if ui.button("Follow").clicked() {
                    let _ = self.bridge.follow(&pk);
                }
            });
            ui.add_space(4.0);
        }

        ui.separator();
        ui.add_space(4.0);

        let Some(author_feed) = feed else {
            ui.label("Loading posts…");
            return;
        };

        let mut nav: Option<AppTab> = None;
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for entry in &author_feed.cards {
                    if let Some(target) = feed_card(
                        ui,
                        &entry.card,
                        &profiles,
                        &snap.embeds,
                        &mut nav,
                        &mut self.reply_to,
                        &self.bridge,
                    ) {
                        self.zap_amount.open(target);
                    }
                    ui.add_space(4.0);
                }
            });
        // ADR-0063 (#1671 Lane F): an author A→B / author→thread click routes
        // through the chokepoint so A's profile.card ref is released first.
        if let Some(next) = nav {
            self.navigate_to(next);
        }
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

#[cfg(test)]
mod ref_lifecycle_tests {
    use super::*;
    use crate::snapshot::{RootCard, TimelineEventCard};
    use std::collections::HashSet;

    fn feed_with_cards(cards: Vec<TimelineEventCard>) -> ModularTimelineSnapshot {
        ModularTimelineSnapshot {
            cards: cards.into_iter().map(|card| RootCard { card }).collect(),
            page: None,
        }
    }

    fn npub_mention(pubkey_hex: &str) -> String {
        let npub = nmp_core::nip19::encode_npub(pubkey_hex).expect("fixture npub encodes");
        format!("hi nostr:{npub}")
    }

    // ── Fix 1: rendered note-body mentions resolve (BLOCKING) ───────────────

    #[test]
    fn body_mention_of_non_author_is_in_visible_set() {
        let author = "a".repeat(64);
        let mentioned = "b".repeat(64);
        let feed = feed_with_cards(vec![TimelineEventCard {
            id: "n1".to_string(),
            author_pubkey: author.clone(),
            content: npub_mention(&mentioned),
            ..Default::default()
        }]);

        let mut visible: HashSet<String> = HashSet::new();
        collect_feed_authors(&feed, &mut visible);

        assert!(
            visible.contains(&author),
            "row author must be resolved"
        );
        assert!(
            visible.contains(&mentioned),
            "a non-author pubkey mentioned in the note body must also be resolved \
             (its name renders via mention_label, not the fallback npub)"
        );
    }

    #[test]
    fn scrolling_a_mention_away_drops_it_from_the_visible_set() {
        let author = "a".repeat(64);
        let mentioned = "b".repeat(64);

        // Frame 1: the mention is on screen → resolved (in the visible set).
        let frame1 = feed_with_cards(vec![TimelineEventCard {
            id: "n1".to_string(),
            author_pubkey: author.clone(),
            content: npub_mention(&mentioned),
            ..Default::default()
        }]);
        let mut resolved: HashSet<String> = HashSet::new();
        collect_feed_authors(&frame1, &mut resolved);
        assert!(resolved.contains(&mentioned));

        // Frame 2: that card scrolled away (empty feed) → the mention is no
        // longer visible, so sync_feed_author_refs releases it (the diff
        // `resolved.difference(visible)` now contains it).
        let frame2 = feed_with_cards(vec![]);
        let mut visible: HashSet<String> = HashSet::new();
        collect_feed_authors(&frame2, &mut visible);

        let released: Vec<&String> = resolved.difference(&visible).collect();
        assert!(
            released.contains(&&mentioned),
            "a mention that scrolls off screen must be released (D5 — bounded)"
        );
    }

    // ── Fix 2: open-view ref release on tab transition (BLOCKING) ───────────

    #[test]
    fn author_a_to_author_b_releases_a_then_opens_b() {
        let a = AppTab::Author("a".repeat(64));
        let b = AppTab::Author("b".repeat(64));
        let (close, open) = view_transition(&a, &b).expect("A→B is a real transition");
        assert_eq!(close, ViewRef::Author("a".repeat(64)), "release outgoing A");
        assert_eq!(open, ViewRef::Author("b".repeat(64)), "open incoming B");
    }

    #[test]
    fn author_to_home_releases_author_no_open_view() {
        let a = AppTab::Author("a".repeat(64));
        let (close, open) = view_transition(&a, &AppTab::Home).expect("Author→Home transitions");
        assert_eq!(close, ViewRef::Author("a".repeat(64)), "release A on leave");
        assert_eq!(open, ViewRef::None, "Home holds no open-profile/thread ref");
    }

    #[test]
    fn author_to_settings_releases_author() {
        let a = AppTab::Author("a".repeat(64));
        let (close, open) =
            view_transition(&a, &AppTab::Settings).expect("Author→Settings transitions");
        assert_eq!(
            close,
            ViewRef::Author("a".repeat(64)),
            "leaving the author view for Settings must release A (no leak)"
        );
        assert_eq!(open, ViewRef::None);
    }

    #[test]
    fn author_to_thread_releases_author_and_opens_thread() {
        let a = AppTab::Author("a".repeat(64));
        let t = AppTab::Thread("evt".to_string());
        let (close, open) = view_transition(&a, &t).expect("Author→Thread transitions");
        assert_eq!(close, ViewRef::Author("a".repeat(64)));
        assert_eq!(open, ViewRef::Thread("evt".to_string()));
    }

    #[test]
    fn thread_to_author_releases_thread_and_opens_author() {
        let t = AppTab::Thread("evt".to_string());
        let a = AppTab::Author("a".repeat(64));
        let (close, open) = view_transition(&t, &a).expect("Thread→Author transitions");
        assert_eq!(close, ViewRef::Thread("evt".to_string()));
        assert_eq!(open, ViewRef::Author("a".repeat(64)));
    }

    #[test]
    fn same_tab_is_a_noop_no_spurious_release() {
        let a = AppTab::Author("a".repeat(64));
        assert!(
            view_transition(&a, &a.clone()).is_none(),
            "re-selecting the same author must NOT release/re-open (no churn)"
        );
        assert!(view_transition(&AppTab::Home, &AppTab::Home).is_none());
    }

    #[test]
    fn home_to_author_opens_b_with_nothing_to_release() {
        let b = AppTab::Author("b".repeat(64));
        let (close, open) = view_transition(&AppTab::Home, &b).expect("Home→Author transitions");
        assert_eq!(close, ViewRef::None, "Home held no open-view ref to release");
        assert_eq!(open, ViewRef::Author("b".repeat(64)));
    }
}

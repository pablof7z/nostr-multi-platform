use egui::{Align, Layout, RichText, ScrollArea, TextEdit, Ui};

use crate::app::DesktopApp;
use crate::snapshot::{DmConversation, DmConversationSnapshot, Snapshot};

pub(crate) fn show(app: &mut DesktopApp, ui: &mut Ui, snap: &Snapshot) {
    ui.heading("Direct Messages");
    ui.separator();

    let signed_in = snap.active_account.is_some();
    new_recipient_row(app, ui, signed_in);
    ui.separator();

    let dm_snapshot: Option<DmConversationSnapshot> = snap.projection("nmp.nip17.dm_inbox");
    let conversations = dm_snapshot
        .as_ref()
        .map(|snapshot| snapshot.conversations.as_slice())
        .unwrap_or(&[]);

    ui.columns(2, |cols| {
        conversation_list(app, &mut cols[0], conversations);
        conversation_detail(app, &mut cols[1], conversations, signed_in);
    });
}

fn new_recipient_row(app: &mut DesktopApp, ui: &mut Ui, signed_in: bool) {
    ui.horizontal(|ui| {
        ui.label("To");
        ui.add_enabled(
            signed_in,
            TextEdit::singleline(&mut app.new_dm_pubkey)
                .hint_text("Recipient pubkey")
                .desired_width(f32::INFINITY),
        );

        let recipient = app.new_dm_pubkey.trim();
        if ui
            .add_enabled(signed_in && !recipient.is_empty(), egui::Button::new("Open"))
            .clicked()
        {
            app.selected_dm_pubkey = Some(recipient.to_string());
        }
    });

    if !signed_in {
        ui.label(RichText::new("Sign in to send encrypted direct messages.").small().weak());
    }
}

fn conversation_list(app: &mut DesktopApp, ui: &mut Ui, conversations: &[DmConversation]) {
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if conversations.is_empty() {
                ui.label(RichText::new("No conversations yet").size(15.0).weak());
                return;
            }

            for conv in conversations {
                let is_selected = app.selected_dm_pubkey.as_ref() == Some(&conv.peer_pubkey);
                if ui
                    .selectable_label(is_selected, &conv.peer_display)
                    .clicked()
                {
                    app.selected_dm_pubkey = Some(conv.peer_pubkey.clone());
                }
                ui.separator();
            }
        });
}

fn conversation_detail(
    app: &mut DesktopApp,
    ui: &mut Ui,
    conversations: &[DmConversation],
    signed_in: bool,
) {
    let Some(selected_pubkey) = app.selected_dm_pubkey.clone() else {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(RichText::new("Select or open a conversation").size(14.0).weak());
        });
        return;
    };

    let conversation = conversations
        .iter()
        .find(|conversation| conversation.peer_pubkey == selected_pubkey);
    let title = conversation
        .map(|conversation| conversation.peer_display.as_str())
        .filter(|display| !display.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| nmp_core::display::short_npub(&selected_pubkey));

    ui.vertical(|ui| {
        ui.label(RichText::new(title).strong());
        ui.separator();

        match conversation {
            Some(conversation) => message_list(ui, conversation),
            None => empty_new_conversation(ui),
        }

        ui.add_space(8.0);
        dm_compose_box(app, ui, &selected_pubkey, signed_in);
    });
}

fn message_list(ui: &mut Ui, conversation: &DmConversation) {
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for msg in &conversation.messages {
                let (author_label, color) = if msg.outgoing {
                    ("You", egui::Color32::from_rgb(96, 165, 250))
                } else {
                    (
                        conversation.peer_display.as_str(),
                        egui::Color32::from_rgb(148, 163, 184),
                    )
                };
                ui.label(RichText::new(author_label).small().color(color).strong());
                ui.label(&msg.content);
                ui.separator();
            }
        });
}

fn empty_new_conversation(ui: &mut Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(40.0);
        ui.label(RichText::new("No messages yet").size(14.0).weak());
    });
}

fn dm_compose_box(app: &mut DesktopApp, ui: &mut Ui, selected_pubkey: &str, signed_in: bool) {
    ui.add_enabled(
        signed_in,
        TextEdit::multiline(&mut app.dm_compose)
            .hint_text("Type a message...")
            .desired_rows(2)
            .desired_width(f32::INFINITY),
    );

    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let recipient = selected_pubkey.trim().to_string();
        let content = app.dm_compose.trim().to_string();
        let can_send = signed_in && can_send_dm(&recipient, &content);
        if ui
            .add_enabled(can_send, egui::Button::new("Send"))
            .clicked()
        {
            let _ = app.bridge.send_dm(&recipient, &content);
            app.dm_compose.clear();
        }
    });
}

fn can_send_dm(recipient_pubkey: &str, content: &str) -> bool {
    !recipient_pubkey.trim().is_empty() && !content.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::can_send_dm;

    #[test]
    fn can_send_dm_requires_recipient_and_content() {
        assert!(can_send_dm("recipient", "hello"));
        assert!(!can_send_dm("", "hello"));
        assert!(!can_send_dm("recipient", ""));
        assert!(!can_send_dm("   ", "hello"));
        assert!(!can_send_dm("recipient", "   "));
    }
}

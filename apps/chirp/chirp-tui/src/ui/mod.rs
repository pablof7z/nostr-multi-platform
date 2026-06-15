pub mod chats;
pub mod colors;
pub mod feature_panels;
pub mod groups;
pub mod help;
pub mod home;
pub mod layout;
pub mod nostr_content;
pub mod nostr_user;
pub mod note_details_modal;
mod outbox;
pub mod palette;
pub mod post_detail;
mod post_detail_rich;
pub mod post_list;
pub mod profile_pane;
pub mod relay_panel;
mod relay_settings;
pub mod settings;
mod shared_snapshot_lines;
pub mod wallet;

#[cfg(test)]
mod layout_tests;

//! Reusable iced widgets for Nostr UI surfaces.
//!
//! Each component is a builder-pattern struct that can be rendered into an
//! iced [`Element`]. Components are pure data + draw calls — they hold no interior
//! mutability and do not depend on the NMP kernel.

pub mod user_avatar;
pub mod user_card;
pub mod user_name;
pub mod user_nip05;
pub mod user_npub;

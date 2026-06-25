use nostr::prelude::*;
use nostr::{ClientMessage, EventBuilder, RelayUrl};

use crate::relay::{send_text, Config, RelaySocket};

const AUTH_REQUIRED_PREFIX: &str = "__auth_required__:";

pub fn send_auth(
    socket: &mut RelaySocket,
    config: &Config,
    challenge: &str,
    bytes_sent: &mut usize,
    auths_sent: &mut usize,
) -> Result<(), String> {
    let keys = config.signing_keys()?;
    let relay_url = RelayUrl::parse(&config.relay).map_err(|e| e.to_string())?;
    let event = EventBuilder::auth(challenge, relay_url)
        .sign_with_keys(&keys)
        .map_err(|e| e.to_string())?;
    let text = ClientMessage::auth(event).as_json();
    *bytes_sent += text.len();
    *auths_sent += 1;
    send_text(socket, &text)
}

pub fn auth_required_error(message: &str) -> String {
    format!("{AUTH_REQUIRED_PREFIX}{message}")
}

pub fn take_auth_required(message: &str) -> Option<&str> {
    message.strip_prefix(AUTH_REQUIRED_PREFIX)
}

pub fn strip_auth_required(message: &str) -> &str {
    take_auth_required(message).unwrap_or(message)
}

pub fn append_auth_count(message: String, auths_sent: usize) -> String {
    if auths_sent == 0 {
        message
    } else {
        format!("{message} (auths_sent={auths_sent})")
    }
}

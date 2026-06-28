//! Browser NIP-11 relay-info fetch hook.
//!
//! Native installs `nmp-nip11::Nip11FetchHook`, whose transport is blocking
//! `ureq` on a spawned thread. The browser runtime needs the same substrate
//! effect with browser capabilities: `fetch()` in the Worker, parse into the
//! substrate-generic `RelayInfoDoc`, then post `SetRelayInfo` back through the
//! runtime inbox. The reducer remains the single writer.

#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use nmp_core::CommandSender;
use nmp_core::substrate::{RelayConnectedHook, RelayInfoDoc};
use nmp_core::time::{Duration, Instant};
use serde::Deserialize;

const NIP11_TTL: Duration = Duration::from_secs(300);
const NOSTR_JSON_ACCEPT: &str = "application/nostr+json";
const MAX_RESPONSE_BYTES: usize = 64 * 1024;

#[derive(Debug)]
pub(crate) struct BrowserNip11FetchHook {
    ttl: Duration,
    user_agent: Option<String>,
    last_fetch: Mutex<HashMap<String, Instant>>,
}

impl BrowserNip11FetchHook {
    pub(crate) fn new(user_agent: Option<String>) -> Self {
        Self {
            ttl: NIP11_TTL,
            user_agent,
            last_fetch: Mutex::new(HashMap::new()),
        }
    }

    fn should_fetch(&self, relay_url: &str, now: Instant) -> bool {
        let Ok(mut map) = self.last_fetch.lock() else {
            return false;
        };
        match map.get(relay_url) {
            Some(prev) if now.saturating_duration_since(*prev) < self.ttl => false,
            _ => {
                map.insert(relay_url.to_string(), now);
                true
            }
        }
    }
}

impl RelayConnectedHook for BrowserNip11FetchHook {
    fn on_relay_connected(
        &self,
        relay_url: &str,
        _is_reconnect: bool,
        command_sender: CommandSender,
    ) {
        if !self.should_fetch(relay_url, Instant::now()) {
            return;
        }
        spawn_browser_fetch(
            relay_url.to_string(),
            self.user_agent.clone(),
            command_sender,
        );
    }
}

#[cfg(target_arch = "wasm32")]
fn spawn_browser_fetch(
    relay_url: String,
    user_agent: Option<String>,
    command_sender: CommandSender,
) {
    wasm_bindgen_futures::spawn_local(async move {
        let Ok(body) = fetch_nip11_body(&relay_url, user_agent.as_deref()).await else {
            return;
        };
        let Ok(doc) = parse_relay_info(&relay_url, &body) else {
            return;
        };
        if let Some(doc_json) = doc.to_json() {
            command_sender.set_relay_info(relay_url, doc_json);
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_browser_fetch(
    _relay_url: String,
    _user_agent: Option<String>,
    _command_sender: CommandSender,
) {
}

#[cfg(target_arch = "wasm32")]
async fn fetch_nip11_body(relay_url: &str, user_agent: Option<&str>) -> Result<Vec<u8>, String> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    let http_url = http_url_for_relay(relay_url)
        .ok_or_else(|| format!("cannot map relay URL to HTTP URL: {relay_url}"))?;
    let headers = web_sys::Headers::new().map_err(js_err)?;
    headers.set("Accept", NOSTR_JSON_ACCEPT).map_err(js_err)?;
    if let Some(user_agent) = user_agent.filter(|s| !s.trim().is_empty()) {
        let _ = headers.set("User-Agent", user_agent);
    }

    let init = web_sys::RequestInit::new();
    init.set_method("GET");
    init.set_mode(web_sys::RequestMode::Cors);
    init.set_headers(&headers);
    let request = web_sys::Request::new_with_str_and_init(&http_url, &init).map_err(js_err)?;
    let global = js_sys::global();
    let promise = if let Some(scope) = global.dyn_ref::<web_sys::WorkerGlobalScope>() {
        scope.fetch_with_request(&request)
    } else if let Some(window) = global.dyn_ref::<web_sys::Window>() {
        window.fetch_with_request(&request)
    } else {
        return Err("browser fetch global unavailable".to_string());
    };
    let response = JsFuture::from(promise)
        .await
        .map_err(js_err)?
        .dyn_into::<web_sys::Response>()
        .map_err(js_err)?;
    if !response.ok() {
        return Err(format!(
            "NIP-11 GET {http_url} returned status {}",
            response.status()
        ));
    }
    let buffer = JsFuture::from(response.array_buffer().map_err(js_err)?)
        .await
        .map_err(js_err)?;
    let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(format!("NIP-11 GET {http_url} exceeded response cap"));
    }
    Ok(bytes)
}

#[cfg(target_arch = "wasm32")]
fn js_err(value: wasm_bindgen::JsValue) -> String {
    value.as_string().unwrap_or_else(|| format!("{value:?}"))
}

fn http_url_for_relay(relay_url: &str) -> Option<String> {
    let trimmed = relay_url.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("wss://") {
        return Some(format!(
            "https://{}",
            &trimmed[trimmed.len() - rest.len()..]
        ));
    }
    if let Some(rest) = lower.strip_prefix("ws://") {
        return Some(format!("http://{}", &trimmed[trimmed.len() - rest.len()..]));
    }
    if lower.starts_with("https://") || lower.starts_with("http://") {
        return Some(trimmed.to_string());
    }
    None
}

#[derive(Debug, Default, Deserialize)]
struct WireDoc {
    name: Option<String>,
    description: Option<String>,
    icon: Option<String>,
    pubkey: Option<String>,
    contact: Option<String>,
    software: Option<String>,
    version: Option<String>,
    #[serde(default)]
    supported_nips: Vec<serde_json::Value>,
    #[serde(default)]
    limitation: WireLimitation,
    #[serde(default)]
    nip29: WireNip29,
}

#[derive(Debug, Default, Deserialize)]
struct WireLimitation {
    payment_required: Option<bool>,
    auth_required: Option<bool>,
    restricted_writes: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct WireNip29 {
    #[serde(default)]
    subgroups: Option<bool>,
}

fn parse_relay_info(relay_url: &str, body: &[u8]) -> Result<RelayInfoDoc, String> {
    let wire: WireDoc =
        serde_json::from_slice(body).map_err(|e| format!("parse NIP-11 document: {e}"))?;
    let mut feature_flags = BTreeMap::new();
    if wire.nip29.subgroups == Some(true) {
        feature_flags.insert("nip29.subgroups".to_string(), true);
    }
    Ok(RelayInfoDoc {
        url: relay_url.to_string(),
        name: non_empty(wire.name),
        description: non_empty(wire.description),
        icon: non_empty(wire.icon),
        pubkey: non_empty(wire.pubkey),
        contact: non_empty(wire.contact),
        software: non_empty(wire.software),
        version: non_empty(wire.version),
        supported_nips: wire
            .supported_nips
            .iter()
            .filter_map(value_to_nip)
            .collect(),
        limitation_payment_required: wire.limitation.payment_required,
        limitation_auth_required: wire.limitation.auth_required,
        limitation_restricted_writes: wire.limitation.restricted_writes,
        feature_flags,
    })
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn value_to_nip(v: &serde_json::Value) -> Option<u32> {
    match v {
        serde_json::Value::Number(n) => {
            let f = n.as_f64()?;
            if f.is_finite() && f >= 0.0 && f.fract() == 0.0 && f <= f64::from(u32::MAX) {
                Some(f as u32)
            } else {
                None
            }
        }
        serde_json::Value::String(s) => s.trim().parse::<u32>().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_websocket_urls_to_http_urls() {
        assert_eq!(
            http_url_for_relay("wss://relay.example/nostr"),
            Some("https://relay.example/nostr".to_string())
        );
        assert_eq!(
            http_url_for_relay("ws://relay.example"),
            Some("http://relay.example".to_string())
        );
        assert_eq!(http_url_for_relay("relay.example"), None);
    }

    #[test]
    fn parses_relay_info_document() {
        let body = br#"{
            "name": "Relay",
            "icon": "https://relay.example/icon.png",
            "supported_nips": [1, "11", 29],
            "limitation": {"auth_required": true},
            "nip29": {"subgroups": true}
        }"#;
        let doc = parse_relay_info("wss://relay.example", body).expect("parse");
        assert_eq!(doc.url, "wss://relay.example");
        assert_eq!(doc.name.as_deref(), Some("Relay"));
        assert_eq!(doc.icon.as_deref(), Some("https://relay.example/icon.png"));
        assert_eq!(doc.supported_nips, vec![1, 11, 29]);
        assert_eq!(doc.limitation_auth_required, Some(true));
        assert_eq!(doc.feature_flags.get("nip29.subgroups"), Some(&true));
    }

    #[test]
    fn ttl_suppresses_duplicate_fetches() {
        let hook = BrowserNip11FetchHook::new(Some("ua".to_string()));
        let t0 = Instant::now();
        assert!(hook.should_fetch("wss://r", t0));
        assert!(!hook.should_fetch("wss://r", t0 + Duration::from_secs(60)));
        assert!(hook.should_fetch("wss://r", t0 + Duration::from_secs(300)));
    }
}

use std::borrow::Cow;
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::Once;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use nmp_nip77::{EligibleFilter, Reconciler, ReconcilerOutcome};
use nostr::prelude::*;
use nostr::{ClientMessage, Event, EventBuilder, EventId, Filter, RelayMessage};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

use crate::cache::{default_cache_path, hex_to_32, CachedEvent, EventCache};

type RelaySocket = WebSocket<MaybeTlsStream<TcpStream>>;

#[derive(Clone)]
pub struct Config {
    pub relay: String,
    pub filter_json: String,
    pub cache_path: PathBuf,
    pub group: Option<String>,
    publish_secret: Option<String>,
    pub demo_tag: String,
    pub read_budget: Duration,
}

impl Config {
    pub fn new(
        relay: String,
        filter_json: String,
        cache_path: Option<PathBuf>,
        group: Option<String>,
        publish_secret: Option<String>,
    ) -> Self {
        let cache_path = cache_path.unwrap_or_else(|| default_cache_path(&relay, &filter_json));
        Self {
            relay,
            filter_json,
            cache_path,
            group,
            publish_secret,
            demo_tag: "nmpnip77demo".to_string(),
            read_budget: Duration::from_secs(10),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct PlainReport {
    pub events: usize,
    pub bytes_sent: usize,
    pub bytes_received: usize,
    pub elapsed_ms: u128,
}

#[derive(Clone, Debug, Default)]
pub struct NegReport {
    pub local_before: usize,
    pub local_after: usize,
    pub have: usize,
    pub need: usize,
    pub fetched: usize,
    pub rounds: usize,
    pub bytes_sent: usize,
    pub bytes_received: usize,
    pub elapsed_ms: u128,
}

#[derive(Clone, Debug, Default)]
pub struct RunReport {
    pub plain: Option<PlainReport>,
    pub neg: Option<NegReport>,
    pub neg_error: Option<String>,
    pub cache_path: PathBuf,
    pub surface: String,
    pub newest: Vec<CachedEvent>,
}

#[derive(Clone, Debug)]
pub struct PublishReport {
    pub id: String,
    pub pubkey: String,
    pub accepted: bool,
    pub relay_message: String,
}

pub fn run_sync(config: &Config, include_plain: bool) -> Result<RunReport, String> {
    let surface = EligibleFilter::parse(&config.filter_json)
        .map(|filter| format!("{:?}", filter.result_surface()))
        .unwrap_or_else(|e| format!("not NIP-77 eligible: {e}"));
    let plain = include_plain.then(|| run_plain_req(config)).transpose()?;
    let (neg, neg_error) = match run_negentropy(config) {
        Ok(report) => (Some(report), None),
        Err(e) => (None, Some(e)),
    };
    let cache = EventCache::load(&config.cache_path, &config.relay, &config.filter_json);
    Ok(RunReport {
        plain,
        neg,
        neg_error,
        cache_path: config.cache_path.clone(),
        surface,
        newest: cache.newest(12),
    })
}

pub fn clear_cache(config: &Config) -> Result<(), String> {
    let cache = EventCache {
        relay: config.relay.clone(),
        filter_json: config.filter_json.clone(),
        events: Default::default(),
    };
    cache.save(&config.cache_path)
}

pub fn publish_demo_event(config: &Config) -> Result<PublishReport, String> {
    let keys = match config.publish_secret.as_deref() {
        Some(secret) => Keys::parse(secret).map_err(|e| format!("invalid --nsec/secret: {e}"))?,
        None => Keys::generate(),
    };
    let pubkey = keys.public_key().to_hex();
    let content = format!(
        "NMP NIP-77 demo event {} #{tag}",
        now_s(),
        tag = config.demo_tag
    );
    let mut builder =
        EventBuilder::new(Kind::TextNote, content).tag(Tag::hashtag(config.demo_tag.clone()));
    if let Some(group) = &config.group {
        builder = builder.tag(
            Tag::parse(["h".to_string(), group.clone()])
                .map_err(|e| format!("invalid group tag: {e}"))?,
        );
    }
    let event = builder.sign_with_keys(&keys).map_err(|e| e.to_string())?;
    let id = event.id.to_hex();
    let mut socket = open(&config.relay)?;
    let text = ClientMessage::event(event).as_json();
    send_text(&mut socket, &text)?;
    let deadline = Instant::now() + config.read_budget;
    while Instant::now() < deadline {
        let Some(frame) = read_text(&mut socket)? else {
            continue;
        };
        if let Ok(RelayMessage::Ok {
            event_id,
            status,
            message,
        }) = RelayMessage::from_json(&frame)
        {
            if event_id.to_hex() == id {
                return Ok(PublishReport {
                    id,
                    pubkey,
                    accepted: status,
                    relay_message: message.into_owned(),
                });
            }
        }
    }
    Err(format!("timed out waiting for OK for {id}"))
}

fn run_plain_req(config: &Config) -> Result<PlainReport, String> {
    let started = Instant::now();
    let mut socket = open(&config.relay)?;
    let filter = parse_filter(&config.filter_json)?;
    let sub = SubscriptionId::new("nmp-demo-plain");
    let req = ClientMessage::req(sub.clone(), filter).as_json();
    let mut report = PlainReport {
        bytes_sent: req.len(),
        ..PlainReport::default()
    };
    send_text(&mut socket, &req)?;
    let deadline = Instant::now() + config.read_budget;
    while Instant::now() < deadline {
        let Some(frame) = read_text(&mut socket)? else {
            continue;
        };
        report.bytes_received += frame.len();
        match RelayMessage::from_json(&frame) {
            Ok(RelayMessage::Event {
                subscription_id, ..
            }) if *subscription_id == sub => report.events += 1,
            Ok(RelayMessage::EndOfStoredEvents(subscription_id)) if *subscription_id == sub => {
                break
            }
            Ok(RelayMessage::Closed { message, .. }) => {
                return Err(format!("plain REQ closed: {message}"))
            }
            Ok(RelayMessage::Notice(message)) => return Err(format!("relay notice: {message}")),
            _ => {}
        }
    }
    let close = ClientMessage::close(sub).as_json();
    report.bytes_sent += close.len();
    let _ = send_text(&mut socket, &close);
    report.elapsed_ms = started.elapsed().as_millis();
    Ok(report)
}

fn run_negentropy(config: &Config) -> Result<NegReport, String> {
    let started = Instant::now();
    let mut cache = EventCache::load(&config.cache_path, &config.relay, &config.filter_json);
    let local_before = cache.events.len();
    let mut reconciler = Reconciler::client(cache.synced_items()).map_err(|e| e.to_string())?;
    let initial = reconciler.initiate().map_err(|e| e.to_string())?;
    let mut socket = open(&config.relay)?;
    let filter = parse_filter(&config.filter_json)?;
    let sub = SubscriptionId::new("nmp-demo-neg");
    let open = ClientMessage::neg_open(sub.clone(), filter, hex_encode(&initial)).as_json();
    let mut report = NegReport {
        local_before,
        bytes_sent: open.len(),
        ..NegReport::default()
    };
    send_text(&mut socket, &open)?;
    let (have, need) = reconcile_loop(config, &mut socket, &sub, &mut reconciler, &mut report)?;
    report.have = have.len();
    report.need = need.len();
    if !need.is_empty() {
        let fetched = fetch_ids(config, &mut socket, &need, &mut report)?;
        report.fetched = fetched.len();
        for event in fetched {
            cache.events.insert(event.id.clone(), event);
        }
        cache.save(&config.cache_path)?;
    }
    let close = ClientMessage::NegClose {
        subscription_id: Cow::Owned(sub),
    }
    .as_json();
    report.bytes_sent += close.len();
    let _ = send_text(&mut socket, &close);
    report.local_after = cache.events.len();
    report.elapsed_ms = started.elapsed().as_millis();
    Ok(report)
}

fn reconcile_loop(
    config: &Config,
    socket: &mut RelaySocket,
    sub: &SubscriptionId,
    reconciler: &mut Reconciler,
    report: &mut NegReport,
) -> Result<(Vec<[u8; 32]>, Vec<[u8; 32]>), String> {
    let deadline = Instant::now() + config.read_budget;
    while Instant::now() < deadline {
        let Some(frame) = read_text(socket)? else {
            continue;
        };
        report.bytes_received += frame.len();
        if let Some(message) = neg_error_message(&frame, sub) {
            return Err(message);
        }
        match RelayMessage::from_json(&frame) {
            Ok(RelayMessage::NegMsg {
                subscription_id,
                message,
            }) if *subscription_id == *sub => {
                report.rounds += 1;
                let payload = hex_decode(&message)?;
                match reconciler.reconcile(&payload).map_err(|e| e.to_string())? {
                    ReconcilerOutcome::Send(next) => {
                        let text = ClientMessage::NegMsg {
                            subscription_id: Cow::Owned(sub.clone()),
                            message: Cow::Owned(hex_encode(&next)),
                        }
                        .as_json();
                        report.bytes_sent += text.len();
                        send_text(socket, &text)?;
                    }
                    ReconcilerOutcome::Done { have, need } => return Ok((have, need)),
                }
            }
            Ok(RelayMessage::NegErr { message, .. }) => return Err(format!("NEG-ERR: {message}")),
            Ok(RelayMessage::Notice(message)) => return Err(format!("relay notice: {message}")),
            Ok(RelayMessage::Closed { message, .. }) => {
                return Err(format!("NEG closed: {message}"))
            }
            _ => {}
        }
    }
    Err("timed out waiting for NIP-77 reconciliation".to_string())
}

fn neg_error_message(frame: &str, sub: &SubscriptionId) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(frame).ok()?;
    let array = value.as_array()?;
    let label = array.first()?.as_str()?;
    if label != "NEG-ERR" && label != "NEG-ERROR" {
        return None;
    }
    let frame_sub = array.get(1)?.as_str()?;
    if frame_sub != sub.to_string() {
        return None;
    }
    let message = array
        .get(2)
        .and_then(|v| v.as_str())
        .unwrap_or("relay returned a NIP-77 error");
    Some(format!("{label}: {message}"))
}

fn fetch_ids(
    config: &Config,
    socket: &mut RelaySocket,
    ids: &[[u8; 32]],
    report: &mut NegReport,
) -> Result<Vec<CachedEvent>, String> {
    let sub = SubscriptionId::new("nmp-demo-ids");
    let event_ids = ids.iter().copied().map(EventId::from_byte_array);
    let req = ClientMessage::req(sub.clone(), Filter::new().ids(event_ids)).as_json();
    report.bytes_sent += req.len();
    send_text(socket, &req)?;
    let mut events = Vec::new();
    let deadline = Instant::now() + config.read_budget;
    while Instant::now() < deadline {
        let Some(frame) = read_text(socket)? else {
            continue;
        };
        report.bytes_received += frame.len();
        match RelayMessage::from_json(&frame) {
            Ok(RelayMessage::Event {
                subscription_id,
                event,
            }) if *subscription_id == sub => events.push(cached_event(&event)),
            Ok(RelayMessage::EndOfStoredEvents(subscription_id)) if *subscription_id == sub => {
                break
            }
            Ok(RelayMessage::Closed { message, .. }) => {
                return Err(format!("ids REQ closed: {message}"))
            }
            _ => {}
        }
    }
    let close = ClientMessage::close(sub).as_json();
    report.bytes_sent += close.len();
    let _ = send_text(socket, &close);
    Ok(events)
}

fn cached_event(event: &Event) -> CachedEvent {
    CachedEvent {
        id: event.id.to_hex(),
        created_at: event.created_at.as_secs(),
        kind: event.kind.as_u16(),
        pubkey: event.pubkey.to_hex(),
        content: event.content.chars().take(96).collect(),
        raw_json: event.as_json(),
    }
}

fn parse_filter(filter_json: &str) -> Result<Filter, String> {
    serde_json::from_str(filter_json).map_err(|e| format!("invalid NIP-01 filter JSON: {e}"))
}

fn open(url: &str) -> Result<RelaySocket, String> {
    install_rustls_provider();
    use tungstenite::client::IntoClientRequest;
    let mut request = url
        .into_client_request()
        .map_err(|e| format!("invalid relay URL: {e}"))?;
    request.headers_mut().insert(
        "User-Agent",
        tungstenite::http::HeaderValue::from_static("nmp-nip77-real-relay-tui/0.1"),
    );
    let (mut socket, _) = connect(request).map_err(|e| e.to_string())?;
    apply_read_timeout(&mut socket);
    Ok(socket)
}

fn install_rustls_provider() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

fn apply_read_timeout(socket: &mut RelaySocket) {
    match socket.get_mut() {
        MaybeTlsStream::Plain(s) => {
            let _ = s.set_read_timeout(Some(Duration::from_millis(250)));
        }
        MaybeTlsStream::Rustls(s) => {
            let _ = s
                .get_ref()
                .set_read_timeout(Some(Duration::from_millis(250)));
        }
        #[allow(unreachable_patterns)]
        _ => {}
    }
}

fn read_text(socket: &mut RelaySocket) -> Result<Option<String>, String> {
    match socket.read() {
        Ok(Message::Text(text)) => Ok(Some(text)),
        Ok(Message::Ping(payload)) => {
            let _ = socket.send(Message::Pong(payload));
            Ok(None)
        }
        Ok(Message::Close(_)) => Err("relay closed websocket".to_string()),
        Ok(_) => Ok(None),
        Err(tungstenite::Error::Io(e))
            if matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) =>
        {
            Ok(None)
        }
        Err(e) => Err(e.to_string()),
    }
}

fn send_text(socket: &mut RelaySocket, text: &str) -> Result<(), String> {
    socket
        .send(Message::Text(text.to_string()))
        .map_err(|e| e.to_string())
}

fn now_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn hex_encode(bytes: &[u8]) -> String {
    static HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    if !s.len().is_multiple_of(2) {
        return Err("odd-length hex payload".to_string());
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for pair in s.as_bytes().chunks(2) {
        let byte = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
        out.push(byte);
    }
    Ok(out)
}

fn hex_nibble(b: u8) -> Result<u8, String> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err("invalid hex nibble".to_string()),
    }
}

pub fn id_prefix(id: &str) -> &str {
    id.get(..12).unwrap_or(id)
}

pub fn parsed_cache_count(config: &Config) -> usize {
    EventCache::load(&config.cache_path, &config.relay, &config.filter_json)
        .events
        .len()
}

#[allow(dead_code)]
fn _assert_hex_to_32_is_used_for_cache_ids(id: &str) -> Option<[u8; 32]> {
    hex_to_32(id)
}

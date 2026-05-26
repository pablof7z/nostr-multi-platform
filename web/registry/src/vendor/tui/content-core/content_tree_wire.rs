use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentTreeWire {
    pub nodes: Vec<WireNode>,
    pub roots: Vec<usize>,
    pub mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireNode {
    Text(String),
    Mention(WireUri),
    EventRef(WireUri),
    Hashtag(String),
    Url(String),
    Media {
        urls: Vec<String>,
        kind: String,
    },
    Emoji {
        shortcode: String,
        url: Option<String>,
    },
    Invoice {
        invoice: WireInvoice,
    },
    Paragraph {
        children: Vec<usize>,
    },
    Heading {
        level: u8,
        children: Vec<usize>,
    },
    BlockQuote {
        children: Vec<usize>,
    },
    CodeBlock {
        info: Option<String>,
        body: String,
    },
    List {
        ordered_start: Option<u64>,
        items: Vec<Vec<usize>>,
    },
    Rule,
    Emphasis {
        children: Vec<usize>,
    },
    Strong {
        children: Vec<usize>,
    },
    InlineCode(String),
    Link {
        children: Vec<usize>,
        href: Option<String>,
    },
    Image {
        alt: String,
        title: Option<String>,
        src: Option<String>,
    },
    SoftBreak,
    HardBreak,
    Placeholder {
        reason: String,
    },
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireUri {
    pub uri: String,
    pub kind: String,
    pub primary_id: String,
    pub relays: Vec<String>,
    pub author: Option<String>,
    pub event_kind: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireInvoice {
    pub kind: String,
    pub value: String,
}

impl ContentTreeWire {
    pub fn from_value(value: &Value) -> Option<Self> {
        let nodes = value
            .get("nodes")?
            .as_array()?
            .iter()
            .map(WireNode::from_value)
            .collect::<Vec<_>>();
        let roots = value
            .get("roots")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|value| value.as_u64().and_then(|idx| usize::try_from(idx).ok()))
            .collect::<Vec<_>>();
        Some(Self {
            nodes,
            roots,
            mode: value
                .get("mode")
                .and_then(Value::as_str)
                .map(str::to_string),
        })
    }

    pub fn mentioned_pubkeys(&self) -> Vec<String> {
        let mut out = std::collections::BTreeSet::new();
        for node in &self.nodes {
            if let WireNode::Mention(uri) = node {
                if is_hex_id_64(&uri.primary_id) {
                    out.insert(uri.primary_id.clone());
                }
            }
        }
        out.into_iter().collect()
    }

    pub fn event_ref_ids(&self) -> Vec<String> {
        let mut out = std::collections::BTreeSet::new();
        for node in &self.nodes {
            if let WireNode::EventRef(uri) = node {
                if uri.kind == "event" && is_hex_id_64(&uri.primary_id) {
                    out.insert(uri.primary_id.clone());
                }
            }
        }
        out.into_iter().collect()
    }

    pub fn media_urls(&self) -> Vec<String> {
        let mut out = Vec::new();
        for node in &self.nodes {
            match node {
                WireNode::Media { urls, .. } => {
                    for url in urls {
                        push_unique(&mut out, url);
                    }
                }
                WireNode::Image { src: Some(src), .. } => push_unique(&mut out, src),
                _ => {}
            }
        }
        out
    }

    pub fn node(&self, index: usize) -> Option<&WireNode> {
        self.nodes.get(index)
    }

    pub fn inline_text(&self, children: &[usize]) -> String {
        children
            .iter()
            .filter_map(|idx| self.node(*idx))
            .map(|node| node.inline_label(self))
            .collect::<Vec<_>>()
            .join("")
    }
}

impl WireNode {
    fn from_value(value: &Value) -> Self {
        match value
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "text" => Self::Text(string(value, "text")),
            "mention" => uri(value).map_or(Self::Unsupported, Self::Mention),
            "event_ref" => uri(value).map_or(Self::Unsupported, Self::EventRef),
            "hashtag" => Self::Hashtag(string(value, "tag")),
            "url" => Self::Url(string(value, "url")),
            "media" => Self::Media {
                urls: strings(value, "urls"),
                kind: string(value, "media_kind"),
            },
            "emoji" => Self::Emoji {
                shortcode: string(value, "shortcode"),
                url: value.get("url").and_then(Value::as_str).map(str::to_string),
            },
            "invoice" => {
                invoice(value).map_or(Self::Unsupported, |invoice| Self::Invoice { invoice })
            }
            "paragraph" => Self::Paragraph {
                children: indices(value, "children"),
            },
            "heading" => Self::Heading {
                level: value.get("level").and_then(Value::as_u64).unwrap_or(1) as u8,
                children: indices(value, "children"),
            },
            "block_quote" => Self::BlockQuote {
                children: indices(value, "children"),
            },
            "code_block" => Self::CodeBlock {
                info: value
                    .get("info")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                body: string(value, "body"),
            },
            "list" => Self::List {
                ordered_start: value.get("ordered_start").and_then(Value::as_u64),
                items: value
                    .get("items")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(indices_array)
                    .collect(),
            },
            "rule" => Self::Rule,
            "emphasis" => Self::Emphasis {
                children: indices(value, "children"),
            },
            "strong" => Self::Strong {
                children: indices(value, "children"),
            },
            "inline_code" => Self::InlineCode(string(value, "code")),
            "link" => Self::Link {
                children: indices(value, "children"),
                href: value
                    .get("href")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            },
            "image" => Self::Image {
                alt: string(value, "alt"),
                title: value
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                src: value.get("src").and_then(Value::as_str).map(str::to_string),
            },
            "soft_break" => Self::SoftBreak,
            "hard_break" => Self::HardBreak,
            "placeholder" => Self::Placeholder {
                reason: string(value, "reason"),
            },
            _ => Self::Unsupported,
        }
    }

    pub fn inline_label(&self, tree: &ContentTreeWire) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Mention(uri) => format!("@{}", short_id(&uri.primary_id)),
            Self::EventRef(uri) => format!("nostr:{}", short_id(&uri.primary_id)),
            Self::Hashtag(tag) => format!("#{tag}"),
            Self::Url(url) => url.clone(),
            Self::Emoji { shortcode, .. } => format!(":{shortcode}:"),
            Self::Invoice { invoice } => {
                format!("[{} invoice]", invoice.kind.to_ascii_lowercase())
            }
            Self::InlineCode(code) => format!("`{code}`"),
            Self::Paragraph { children }
            | Self::Heading { children, .. }
            | Self::BlockQuote { children } => tree.inline_text(children),
            Self::List {
                ordered_start,
                items,
            } => items
                .iter()
                .enumerate()
                .map(|(idx, item)| {
                    let marker = ordered_start
                        .map(|start| format!("{}.", start + idx as u64))
                        .unwrap_or_else(|| "-".to_string());
                    format!("{marker} {}", tree.inline_text(item))
                })
                .collect::<Vec<_>>()
                .join("\n"),
            Self::CodeBlock { body, .. } => body.clone(),
            Self::Media { urls, kind } => {
                format!("[{} media: {}]", kind.to_ascii_lowercase(), urls.len())
            }
            Self::Emphasis { children } | Self::Strong { children } => tree.inline_text(children),
            Self::Link { children, href } => {
                let label = tree.inline_text(children);
                if label.is_empty() {
                    href.clone().unwrap_or_default()
                } else {
                    label
                }
            }
            Self::SoftBreak => " ".to_string(),
            Self::HardBreak => "\n".to_string(),
            Self::Image { alt, title, .. } => title
                .as_ref()
                .map(|title| format!("[image: {alt} - {title}]"))
                .unwrap_or_else(|| format!("[image: {alt}]")),
            Self::Placeholder { reason } => format!("[{reason}]"),
            _ => String::new(),
        }
    }
}

fn uri(value: &Value) -> Option<WireUri> {
    let uri = value.get("uri")?;
    Some(WireUri {
        uri: string(uri, "uri"),
        kind: string(uri, "kind"),
        primary_id: string(uri, "primary_id"),
        relays: strings(uri, "relays"),
        author: uri
            .get("author")
            .and_then(Value::as_str)
            .map(str::to_string),
        event_kind: uri.get("event_kind").and_then(Value::as_u64),
    })
}

fn invoice(value: &Value) -> Option<WireInvoice> {
    let object = value.get("invoice")?.as_object()?;
    let (kind, payload) = object.iter().next()?;
    Some(WireInvoice {
        kind: kind.clone(),
        value: payload.as_str().unwrap_or_default().to_string(),
    })
}

fn string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn strings(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn indices(value: &Value, key: &str) -> Vec<usize> {
    value.get(key).map(indices_array).unwrap_or_default()
}

fn indices_array(value: &Value) -> Vec<usize> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_u64().and_then(|idx| usize::try_from(idx).ok()))
        .collect()
}

fn is_hex_id_64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn push_unique(out: &mut Vec<String>, value: &str) {
    if !value.is_empty() && !out.iter().any(|existing| existing == value) {
        out.push(value.to_string());
    }
}

fn short_id(id: &str) -> String {
    let count = id.chars().count();
    if count <= 12 {
        id.to_string()
    } else {
        let head = id.chars().take(6).collect::<String>();
        let tail = id
            .chars()
            .rev()
            .take(6)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<String>();
        format!("{head}…{tail}")
    }
}

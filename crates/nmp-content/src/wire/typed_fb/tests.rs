use super::*;

fn round_trip(tree: &ContentTreeWire) -> ContentTreeWire {
    let bytes = encode_content_tree(tree);
    decode_content_tree(&bytes).expect("decode must succeed")
}

#[test]
fn content_tree_round_trips_simple_text() {
    let tree = ContentTreeWire {
        nodes: vec![WireNode::Text {
            text: "hello world".to_string(),
        }],
        roots: vec![0],
        mode: RenderMode::Plain,
    };
    assert_eq!(round_trip(&tree), tree);
}

#[test]
fn content_tree_round_trips_with_event_ref() {
    let tree = ContentTreeWire {
        nodes: vec![WireNode::EventRef {
            uri: WireNostrUri {
                uri: "nostr:nevent1qqq".to_string(),
                kind: WireNostrUriKind::Event,
                primary_id: "deadbeef".to_string(),
                relays: vec!["wss://relay.example".to_string()],
                author: Some("cafebabe".to_string()),
                event_kind: Some(1),
            },
        }],
        roots: vec![0],
        mode: RenderMode::Plain,
    };
    assert_eq!(round_trip(&tree), tree);
}

#[test]
fn content_tree_round_trips_nested_paragraph() {
    // Arena: [0]=Paragraph{children:[1,2]}, [1]=Text, [2]=Strong{children:[3]}, [3]=Text
    let tree = ContentTreeWire {
        nodes: vec![
            WireNode::Paragraph {
                children: vec![1, 2],
            },
            WireNode::Text {
                text: "lead ".to_string(),
            },
            WireNode::Strong { children: vec![3] },
            WireNode::Text {
                text: "bold".to_string(),
            },
        ],
        roots: vec![0],
        mode: RenderMode::Markdown,
    };
    assert_eq!(round_trip(&tree), tree);
}

#[test]
fn file_identifier_is_nfct() {
    let tree = ContentTreeWire::default();
    let bytes = encode_content_tree(&tree);
    // Bytes 4..8 hold the file identifier in a finished FlatBuffer.
    assert_eq!(&bytes[4..8], FILE_IDENTIFIER);
    assert!(fb::content_tree_wire_buffer_has_identifier(&bytes));
}

#[test]
fn round_trips_every_node_kind() {
    let tree = ContentTreeWire {
        nodes: vec![
            WireNode::Text {
                text: "t".to_string(),
            },
            WireNode::Mention {
                uri: WireNostrUri {
                    uri: "nostr:npub1".to_string(),
                    kind: WireNostrUriKind::Profile,
                    primary_id: "pk".to_string(),
                    relays: vec![],
                    author: None,
                    event_kind: None,
                },
            },
            WireNode::Hashtag {
                tag: "nostr".to_string(),
            },
            WireNode::Url {
                url: "https://example.com".to_string(),
            },
            WireNode::Media {
                urls: vec!["https://a.png".to_string(), "https://b.png".to_string()],
                media_kind: MediaKind::Image,
            },
            WireNode::Emoji {
                shortcode: "smile".to_string(),
                url: Some("https://e.png".to_string()),
            },
            WireNode::Invoice {
                invoice: InvoiceKind::Bolt11("lnbc1".to_string()),
            },
            WireNode::Heading {
                level: 2,
                children: vec![0],
            },
            WireNode::BlockQuote { children: vec![0] },
            WireNode::CodeBlock {
                info: Some("rust".to_string()),
                body: "fn main() {}".to_string(),
            },
            WireNode::List {
                ordered_start: Some(3),
                items: vec![vec![0], vec![2, 3]],
            },
            WireNode::Rule,
            WireNode::Emphasis { children: vec![0] },
            WireNode::InlineCode {
                code: "let x = 1;".to_string(),
            },
            WireNode::Link {
                children: vec![0],
                href: Some("https://link".to_string()),
            },
            WireNode::Image {
                alt: "alt".to_string(),
                title: Some("title".to_string()),
                src: Some("https://img".to_string()),
            },
            WireNode::SoftBreak,
            WireNode::HardBreak,
            WireNode::Placeholder {
                reason: PlaceholderReason::UnresolvedUri,
            },
        ],
        roots: vec![0],
        mode: RenderMode::Markdown,
    };
    assert_eq!(round_trip(&tree), tree);
}

#[test]
fn list_unordered_round_trips_as_none() {
    let tree = ContentTreeWire {
        nodes: vec![WireNode::List {
            ordered_start: None,
            items: vec![vec![]],
        }],
        roots: vec![0],
        mode: RenderMode::Markdown,
    };
    let back = round_trip(&tree);
    match &back.nodes[0] {
        WireNode::List { ordered_start, .. } => assert_eq!(*ordered_start, None),
        other => panic!("expected List, got {other:?}"),
    }
}

#[test]
fn optional_fields_absent_round_trip_as_none() {
    let tree = ContentTreeWire {
        nodes: vec![
            WireNode::Emoji {
                shortcode: "x".to_string(),
                url: None,
            },
            WireNode::CodeBlock {
                info: None,
                body: "code".to_string(),
            },
            WireNode::Link {
                children: vec![],
                href: None,
            },
            WireNode::Image {
                alt: "a".to_string(),
                title: None,
                src: None,
            },
        ],
        roots: vec![0, 1, 2, 3],
        mode: RenderMode::Markdown,
    };
    assert_eq!(round_trip(&tree), tree);
}

#[test]
fn invoice_kinds_round_trip() {
    for invoice in [
        InvoiceKind::Bolt11("lnbc".to_string()),
        InvoiceKind::Bolt12("lno".to_string()),
        InvoiceKind::Cashu("cashuA".to_string()),
    ] {
        let tree = ContentTreeWire {
            nodes: vec![WireNode::Invoice {
                invoice: invoice.clone(),
            }],
            roots: vec![0],
            mode: RenderMode::Plain,
        };
        assert_eq!(round_trip(&tree).nodes[0], WireNode::Invoice { invoice });
    }
}

#[test]
fn render_mode_variants_round_trip() {
    for mode in [RenderMode::Auto, RenderMode::Plain, RenderMode::Markdown] {
        let tree = ContentTreeWire {
            nodes: vec![],
            roots: vec![],
            mode,
        };
        assert_eq!(round_trip(&tree).mode, mode);
    }
}

#[test]
fn event_kind_some_zero_and_none_round_trip_distinctly() {
    // `Some(0)` must NOT collapse to `None` (regression guard for the
    // `EVENT_KIND_NONE` sentinel).
    for event_kind in [None, Some(0u32), Some(1u32), Some(65535u32)] {
        let tree = ContentTreeWire {
            nodes: vec![WireNode::Mention {
                uri: WireNostrUri {
                    uri: "nostr:npub1".to_string(),
                    kind: WireNostrUriKind::Profile,
                    primary_id: "pk".to_string(),
                    relays: vec![],
                    author: None,
                    event_kind,
                },
            }],
            roots: vec![0],
            mode: RenderMode::Plain,
        };
        let back = round_trip(&tree);
        match &back.nodes[0] {
            WireNode::Mention { uri } => assert_eq!(uri.event_kind, event_kind),
            other => panic!("expected Mention, got {other:?}"),
        }
    }
}

#[test]
fn decode_rejects_garbage() {
    let err = decode_content_tree(&[0u8; 3]);
    assert!(err.is_err());
}

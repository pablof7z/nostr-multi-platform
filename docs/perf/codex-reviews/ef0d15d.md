# Codex post-merge review — ef0d15d (T78 nmp-content Layer A)

Reviewed merge: `56901c9` (fix(test)) + `ef0d15d` (feat(content): nmp-content Layer A).
Codex applied FIX-IN-PLACE as `bf5a507` and pushed. Capture below is the
codex stdout tail (streamed; head truncated by backgrounding — the
load-bearing FIX + REPORT sections are intact). 5 REPORT-class
follow-ups for orchestrator dispatch.

---

-        fn prop_nprofile_round_trip(
-            hex in hex32(),
-            relays in proptest::collection::vec(relay_url(), 0..=3)
-        ) {
-            let data = NprofileData { pubkey: hex, relays };
-            let bech = encode_nprofile(&data).unwrap();
-            prop_assert_eq!(decode_nprofile(&bech).unwrap(), data);
-        }
-
-        #[test]
-        fn prop_nevent_round_trip(
-            hex in hex32(),
-            author_hex in hex32(),
-            kind in any::<u32>(),
-            relays in proptest::collection::vec(relay_url(), 0..=2)
-        ) {
-            let data = NeventData {
-                event_id: hex,
-                relays,
-                author: Some(author_hex),
-                kind: Some(kind),
-            };
-            let bech = encode_nevent(&data).unwrap();
-            prop_assert_eq!(decode_nevent(&bech).unwrap(), data);
-        }
-
-        #[test]
-        fn prop_naddr_round_trip(
-            id in "[a-z0-9-]{0,40}",
-            hex in hex32(),
-            kind in any::<u32>(),
-            relays in proptest::collection::vec(relay_url(), 0..=2)
-        ) {
-            let data = NaddrData { identifier: id, pubkey: hex, kind, relays };
-            let bech = encode_naddr(&data).unwrap();
-            prop_assert_eq!(decode_naddr(&bech).unwrap(), data);
-        }
-    }
-}

tokens used
140,259
**FIX**
Pushed `bf5a5076ddfdfe2117be33995a7b02bdac6adb23` to `master`.

Fixed:
- Split hard-cap file: `crates/nmp-core/tests/nip19_nip21.rs` `514 -> 450` LOC; moved proptests to `nip19_nip21_props.rs`.
- Corrected stale/contradictory docs in `RenderContext` helper and `nmp-content` D0 comment.

Verified:
- `cargo test -p nmp-content`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test -p nmp-core --test nip19_nip21 --test nip19_nip21_props`

**REPORT**
- [embed_registry.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-content/src/embed_registry.rs:204): `release` ignores `handle_id`; double-release can decrement another live claim. Add active handle IDs per entry and double-release tests.
- [embed_registry.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-content/src/embed_registry.rs:341): replacement drops the removal delta; [line 327](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-content/src/embed_registry.rs:327) also only clears `Event` targets, not address claims. Needs D8 delta semantics/tests.
- [tokenizer.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-content/src/tokenizer.rs:140): tokenizer can drop source text: hashtag matching consumes leading whitespace, and [URL trimming](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-content/src/tokenizer.rs:203) drops trailing punctuation. Add round-trip rendering tests, then preserve prefix/suffix text.
- [markdown.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-content/src/markdown.rs:102): enables table/strikethrough despite PD-012 CommonMark-only. [Image handling](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-content/src/markdown.rs:208) also uses title as alt and leaks alt text. Needs markdown behavior tests/design call.
- Soft LOC follow-up: [embed_registry.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-content/src/embed_registry.rs:1) 490, [markdown.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-content/src/markdown.rs:1) 449, [tokenizer.rs](/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-content/src/tokenizer.rs:1) 344. Split before adding more behavior.

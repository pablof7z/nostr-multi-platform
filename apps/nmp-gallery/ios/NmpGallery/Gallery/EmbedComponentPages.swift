import SwiftUI

/// Showcase pages for the kind-dispatch embed renderers (ADR-0034 / M16).
///
/// Each page builds a `ContentTreeWire` containing surrounding prose plus an
/// `EventRef` for a real bech32 URI. `NostrContentView` walks the tree; on
/// hitting the `EventRef` it instantiates `EmbeddedEvent(uri: …)` which fires
/// `sink.claim(uri, consumerId)` via `task(id:)`. The kernel resolves the
/// event (cache or relay), surfaces it in `projections.claimed_events`, and
/// `EmbedHost.update(fromSnapshotJSON:)` decodes the typed envelope. SwiftUI
/// re-evaluates and the registry-resolved renderer paints the result.
///
/// Mirrors the TUI showcase in `apps/nmp-gallery/tui/src/data.rs::from_live`.

/// Shared chrome — copied from `ContentComponentPages.swift` so the layout
/// stays in sync.
private struct EmbedPageFrame<Content: View>: View {
    let caption: String
    @ViewBuilder var content: () -> Content

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(caption)
                .font(.caption)
                .foregroundStyle(.secondary)
            VStack(alignment: .leading) {
                content()
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(20)
            .background(Color(.secondarySystemGroupedBackground))
            .clipShape(RoundedRectangle(cornerRadius: 12))
        }
    }
}

// MARK: - Article — kind:30023 (Gigi's "What's left of the internet?")

/// Gigi's long-form article naddr. Matches the TUI's `ARTICLE_NADDR`; the
/// kernel's seeded relays (relay.primal.net + purplepag.es) resolve it via
/// the OneshotApi cache or one round-trip.
private let GIGI_ARTICLE_NADDR =
    "nostr:naddr1qvzqqqr4gupzqmjxss3dld622uu8q25gywum9qtg4w4cv4064jmg20xsac2aam5nqy6xsar5wpen5te0v3jhyemfva5jucm0d5hnyvpjxchnqve0xgcz7argv5kkjmn5v4exuet594kx2en594kk2tcqz36xsefdd9h8getjdejhgttvv4n8gttdv55zqsmp"

struct ArticleEmbedPage: View {
    @Environment(GalleryModel.self) private var model

    var body: some View {
        VStack(spacing: 16) {
            EmbedPageFrame(caption: "Article embed — kind:30023 via NostrKindRegistry") {
                NostrContentView(tree: tree)
                Text("The renderer fires `claim` on the article naddr; the kernel resolves kind:30023 and the typed `ArticleProjection` flows through `EmbedHost`.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .padding(.top, 6)
            }
        }
    }

    /// Surrounding prose + article naddr, same shape as the TUI showcase.
    private var tree: ContentTreeWire {
        // Arena:
        //   0  text "hey, check out my article "
        //   1  eventRef(naddr)
        //   2  text " I hope you enjoy it!"
        //   3  paragraph([0, 1, 2])
        ContentTreeWire(
            nodes: [
                .text("hey, check out my article "),
                .eventRef(NostrWireUri(
                    uri: GIGI_ARTICLE_NADDR,
                    kind: .address,
                    primaryId: "30023:6e468422e84cc0c1c879b6207e3e93da3ad7ab86157a8c2fa1467e02de166e22:the-internet-left-me"
                )),
                .text(" I hope you enjoy it!"),
                .paragraph(children: [0, 1, 2]),
            ],
            roots: [3],
            mode: nil
        )
    }
}

// MARK: - Profile — inline npub mention chip

struct ProfileEmbedPage: View {
    @Environment(GalleryModel.self) private var model

    var body: some View {
        VStack(spacing: 16) {
            EmbedPageFrame(caption: "Inline profile mention — kind:0 via mention chip") {
                NostrContentView(
                    tree: tree,
                    mentionLabel: { uri in
                        model.profile(forPubkey: uri.primaryId)?.displayName
                            ?? NostrContentView.defaultMentionLabel(uri)
                    }
                )
                Text("Profile mentions resolve via `projections.mention_profiles` — the same kind:0 path the user-* pages use. No embed claim is required for `npub:` URIs.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .padding(.top, 6)
            }
        }
    }

    private var tree: ContentTreeWire {
        // Arena:
        //   0  text "met "
        //   1  mention(DEMO_PUBKEY)
        //   2  text " at a nostr conference last week, brilliant mind"
        //   3  paragraph([0, 1, 2])
        ContentTreeWire(
            nodes: [
                .text("met "),
                .mention(NostrWireUri(
                    uri: "nostr:\(DEMO_NPUB)",
                    kind: .profile,
                    primaryId: DEMO_PUBKEY_HEX
                )),
                .text(" at a nostr conference last week, brilliant mind"),
                .paragraph(children: [0, 1, 2]),
            ],
            roots: [3],
            mode: nil
        )
    }
}

// MARK: - Note — kind:1 short text note via nevent

/// The gallery's existing MENTION_EVENT_ID (caef905a…). Matches the TUI's
/// reproduction set so both surfaces show the same data.
private let DEMO_NOTE_EVENT_ID =
    "caef905a1e1520fd6621b56364cca823c262327a32ac063b4ff0435f41aa7660"

private let DEMO_NOTE_NEVENT =
    "nostr:nevent1qqsx2alyz6rc4y8avxy9d3keejs3rcnzxx0gevpsa5lcyp475x4nkqgpzpmhxue69uhkummnw3ezuamfdejszrthwden5te0dehhxtnvdakqyfhwden5te0wfjkccte9emxxe33xyenjwfjvgg5kgsdc"

struct NoteEmbedPage: View {
    @Environment(GalleryModel.self) private var model

    var body: some View {
        VStack(spacing: 16) {
            EmbedPageFrame(caption: "Note embed — kind:1 via NostrKindRegistry") {
                NostrContentView(tree: tree)
                Text("nevent1… URIs resolve via the same `claim_event` path. The default short-note renderer paints author + content.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .padding(.top, 6)
            }
        }
    }

    private var tree: ContentTreeWire {
        // Arena:
        //   0  text "this is a great point "
        //   1  eventRef(nevent)
        //   2  text " what do you think?"
        //   3  paragraph([0, 1, 2])
        ContentTreeWire(
            nodes: [
                .text("this is a great point "),
                .eventRef(NostrWireUri(
                    uri: DEMO_NOTE_NEVENT,
                    kind: .event,
                    primaryId: DEMO_NOTE_EVENT_ID
                )),
                .text(" what do you think?"),
                .paragraph(children: [0, 1, 2]),
            ],
            roots: [3],
            mode: nil
        )
    }
}

// MARK: - Highlight — kind:9802 via nevent

/// A canonical NIP-84 highlight event. If the kernel can't resolve it the
/// renderer keeps the loading placeholder visible; the kind:9802 default
/// renderer (overridden by HighlightEmbed) only runs once a real envelope
/// lands in projections.claimed_events.
private let DEMO_HIGHLIGHT_EVENT_ID =
    "2df88accbf264b10f47809abcf9d32b4146b035a5a197c9ff30e45ac010d5368"

private let DEMO_HIGHLIGHT_NEVENT =
    "nostr:nevent1qqstdhz9vewfjtzr685qf40e7n9dq5dvqddtgvhe8luhpyt2qzr2nfcpzpmhxue69uhkummnw3ezuamfdejszrthwden5te0dehhxtnvdakqyfhwden5te0wfjkccte9emxxe33xyenjwfjvgg5kgsdc"

struct HighlightEmbedPage: View {
    @Environment(GalleryModel.self) private var model

    var body: some View {
        VStack(spacing: 16) {
            EmbedPageFrame(caption: "Highlight embed — kind:9802 via HighlightEmbed renderer") {
                NostrContentView(tree: tree)
                Text("NIP-84 highlights render as a pull-quote with optional source link. The kernel resolves kind:9802; `HighlightEmbed` paints the typed projection.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .padding(.top, 6)
            }
        }
    }

    private var tree: ContentTreeWire {
        // Arena:
        //   0  text "found this interesting "
        //   1  eventRef(highlight nevent)
        //   2  paragraph([0, 1])
        ContentTreeWire(
            nodes: [
                .text("found this interesting "),
                .eventRef(NostrWireUri(
                    uri: DEMO_HIGHLIGHT_NEVENT,
                    kind: .event,
                    primaryId: DEMO_HIGHLIGHT_EVENT_ID
                )),
                .paragraph(children: [0, 1]),
            ],
            roots: [2],
            mode: nil
        )
    }
}

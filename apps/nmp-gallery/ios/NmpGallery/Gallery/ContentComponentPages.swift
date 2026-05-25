import SwiftUI

/// Shared chrome for content-component pages.
private struct ContentPageFrame<Content: View>: View {
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

/// Reusable toggle shown at the top of pages that render inline mentions.
/// When on, `NostrContentView` receives the raw wire URI; when off, the
/// kernel-resolved display name (or truncated pubkey while loading).
private struct RawToggle: View {
    @Binding var rawMode: Bool

    var body: some View {
        HStack {
            Text(rawMode ? "Raw wire" : "Resolved")
                .font(.subheadline)
                .foregroundStyle(.secondary)
            Spacer()
            Toggle("", isOn: $rawMode)
                .labelsHidden()
        }
    }
}

// MARK: - Sample data

/// A reusable rich `ContentTreeWire` exercise. Mirrors the registry's
/// `NostrContentViewPreview` arena and adds a media node so the
/// content-media-grid page can reuse it.
private enum SampleContent {
    static var richTree: ContentTreeWire {
        // Arena layout:
        //   0  text "hello "
        //   1  mention(DEMO_PUBKEY_HEX)
        //   2  text " and "
        //   3  hashtag "nostr"
        //   4  text " — "
        //   5  url "https://nmp.dev"
        //   6  paragraph(children: [0,1,2,3,4,5])
        //   7  text "Section"
        //   8  heading(level: 2, children: [7])
        //   9  code_block info=rust body=fn main()
        return ContentTreeWire(
            nodes: [
                .text("hello "),
                .mention(
                    NostrWireUri(
                        uri: "nostr:npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft",
                        kind: .profile,
                        primaryId: DEMO_PUBKEY_HEX
                    )
                ),
                .text(" and "),
                .hashtag("nostr"),
                .text(" — "),
                .url("https://nmp.dev"),
                .paragraph(children: [0, 1, 2, 3, 4, 5]),
                .text("Section"),
                .heading(level: 2, children: [7]),
                .codeBlock(info: "rust", body: "fn main() {}"),
            ],
            roots: [6, 8, 9],
            mode: nil
        )
    }

    static var imageUrls: [URL] {
        [
            URL(string: "https://picsum.photos/seed/nmp1/800/600")!,
            URL(string: "https://picsum.photos/seed/nmp2/800/600")!,
            URL(string: "https://picsum.photos/seed/nmp3/800/600")!,
            URL(string: "https://picsum.photos/seed/nmp4/800/600")!,
        ]
    }
}

// MARK: - content-core

struct ContentCorePage: View {
    var body: some View {
        VStack(spacing: 16) {
            ContentPageFrame(caption: "ContentTreeWire — arena dump") {
                let tree = SampleContent.richTree
                Text("nodes: \(tree.nodes.count)   roots: \(tree.roots.count)")
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
                Text("Render it with NostrContentView; the wire is just data.")
                    .font(.callout)
            }
            ContentPageFrame(caption: "NostrIdenticon.identiconView(forPubkey:size:)") {
                HStack(spacing: 16) {
                    NostrIdenticon.identiconView(forPubkey: "deadbeef0001", size: 48)
                    NostrIdenticon.identiconView(forPubkey: "deadbeef0002", size: 48)
                    NostrIdenticon.identiconView(forPubkey: "deadbeef0003", size: 48)
                    NostrIdenticon.identiconView(forPubkey: "deadbeef0004", size: 48)
                }
            }
        }
    }
}

// MARK: - content-view

struct ContentViewPage: View {
    @Environment(GalleryModel.self) private var model
    @State private var rawMode = false

    var body: some View {
        VStack(spacing: 16) {
            RawToggle(rawMode: $rawMode)
            ContentPageFrame(caption: "NostrContentView(tree:)") {
                NostrContentView(
                    tree: SampleContent.richTree,
                    mentionLabel: { uri in
                        rawMode
                            ? uri.uri
                            : model.profile(forPubkey: uri.primaryId)?.displayName
                                ?? NostrContentView.defaultMentionLabel(uri)
                    }
                )
            }
        }
    }
}

// MARK: - content-mention-chip

/// Inline-mention demo: a `ContentTreeWire` that renders "Hey @pablof7z,
/// how are you?". The mention node's `primaryId` is `DEMO_PUBKEY_HEX`
/// and the `mentionLabel` closure looks up the live kind:0 profile the
/// kernel claimed at startup. Raw toggle shows the wire URI vs the
/// resolved display name.
private enum MentionSample {
    /// Arena layout:
    ///   0  text "Hey "
    ///   1  mention(DEMO_PUBKEY_HEX)
    ///   2  text ", how are you?"
    ///   3  paragraph(children: [0, 1, 2])
    static var note: ContentTreeWire {
        ContentTreeWire(
            nodes: [
                .text("Hey "),
                .mention(
                    NostrWireUri(
                        uri: "nostr:npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft",
                        kind: .profile,
                        primaryId: DEMO_PUBKEY_HEX
                    )
                ),
                .text(", how are you?"),
                .paragraph(children: [0, 1, 2]),
            ],
            roots: [3],
            mode: nil
        )
    }
}

struct ContentMentionChipPage: View {
    @Environment(GalleryModel.self) private var model
    @State private var rawMode = false

    var body: some View {
        let profile = model.profile(forPubkey: DEMO_PUBKEY_HEX)
        VStack(spacing: 16) {
            RawToggle(rawMode: $rawMode)
            ContentPageFrame(caption: "NostrContentView — live mention resolution") {
                NostrContentView(
                    tree: MentionSample.note,
                    mentionLabel: { uri in
                        rawMode
                            ? uri.uri
                            : model.profile(forPubkey: uri.primaryId)?.displayName
                                ?? NostrContentView.defaultMentionLabel(uri)
                    }
                )
                Text("The kernel fetches kind:0 automatically; the app just reads the snapshot.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .padding(.top, 4)
            }
            ContentPageFrame(caption: "NostrMentionChip — kernel-resolved profile") {
                NostrMentionChip(
                    pubkey: DEMO_PUBKEY_HEX,
                    displayName: profile?.displayName,
                    avatarUrl: profile?.avatarURL
                )
            }
            ContentPageFrame(caption: "NostrMentionChip — identicon fallback (unknown pubkey)") {
                NostrMentionChip(
                    pubkey: "deadbeefcafebabedeadbeefcafebabe",
                    displayName: nil
                )
            }
            ContentPageFrame(caption: "NostrMentionChip — no avatar") {
                NostrMentionChip(
                    pubkey: DEMO_PUBKEY_HEX,
                    displayName: profile?.displayName,
                    showsAvatar: false
                )
            }
        }
    }
}

// MARK: - content-minimal

struct ContentMinimalPage: View {
    @Environment(GalleryModel.self) private var model
    @State private var rawMode = false

    var body: some View {
        VStack(spacing: 16) {
            RawToggle(rawMode: $rawMode)
            ContentPageFrame(caption: "NostrMinimalContentView(runs:)") {
                NostrMinimalContentView(runs: runs)
            }
        }
    }

    private var runs: [NostrContentRun] {
        SampleContent.richTree.nostrMinimalRuns(
            mentionLabel: rawMode
                ? { uri in uri.uri }
                : { uri in
                    model.profile(forPubkey: uri.primaryId)?.displayName
                        ?? NostrContentView.defaultMentionLabel(uri)
                }
        )
    }
}

// MARK: - content-media-grid

struct ContentMediaGridPage: View {
    var body: some View {
        VStack(spacing: 16) {
            ContentPageFrame(caption: "1 image") {
                NostrMediaGrid(imageUrls: Array(SampleContent.imageUrls.prefix(1)))
            }
            ContentPageFrame(caption: "2 images") {
                NostrMediaGrid(imageUrls: Array(SampleContent.imageUrls.prefix(2)))
            }
            ContentPageFrame(caption: "3 images") {
                NostrMediaGrid(imageUrls: Array(SampleContent.imageUrls.prefix(3)))
            }
            ContentPageFrame(caption: "4 images (2×2)") {
                NostrMediaGrid(imageUrls: SampleContent.imageUrls)
            }
        }
    }
}

// MARK: - content-quote-card

struct ContentQuoteCardPage: View {
    @Environment(GalleryModel.self) private var model

    var body: some View {
        let profile = model.profile(forPubkey: DEMO_PUBKEY_HEX)
        VStack(spacing: 16) {
            ContentPageFrame(caption: "NostrQuoteCard — rich") {
                NostrQuoteCard(
                    model: NostrQuoteCardModel(
                        id: "demo-event-1",
                        authorPubkey: DEMO_PUBKEY_HEX,
                        authorDisplayName: profile?.displayName,
                        content: "GM Nostr. This is what an embedded note quote card looks like.",
                        createdAtDisplay: "2026-05-25"
                    ),
                    variant: .rich
                )
            }
            ContentPageFrame(caption: "NostrQuoteCard — compact") {
                NostrQuoteCard(
                    model: NostrQuoteCardModel(
                        id: "demo-event-2",
                        authorPubkey: DEMO_PUBKEY_HEX,
                        authorDisplayName: profile?.displayName,
                        content: "Compact card variant — single-line attribution + truncated body."
                    ),
                    variant: .compact
                )
            }
            ContentPageFrame(caption: "NostrQuoteCard — collapsed") {
                NostrQuoteCard(
                    model: NostrQuoteCardModel(
                        id: "demo-event-3",
                        unresolvedUri: "nostr:nevent1example"
                    ),
                    variant: .collapsed
                )
            }
            ContentPageFrame(caption: "NostrQuoteCard — missing") {
                NostrQuoteCard(
                    model: NostrQuoteCardModel(
                        id: "missing",
                        unresolvedUri: "nostr:nevent1unresolved"
                    ),
                    variant: .missing
                )
            }
        }
    }
}

import SwiftUI

/// Hand-built `ContentTreeWire` exercise for previews. Constructs the arena
/// directly rather than decoding JSON, so the preview doesn't need fixtures.
struct NostrContentViewPreview: View {
    var body: some View {
        // Arena layout:
        //   0  text "hello "
        //   1  mention(fa984b…018f52)
        //   2  text " and "
        //   3  hashtag "nostr"
        //   4  text " — "
        //   5  url "https://nmp.dev"
        //   6  paragraph(children: [0,1,2,3,4,5])
        //   7  text "Section"
        //   8  heading(level: 2, children: [7])
        //   9  code_block info=rust body=fn main()
        let tree = ContentTreeWire(
            nodes: [
                .text("hello "),
                .mention(
                    NostrWireUri(
                        uri: "nostr:npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft",
                        kind: .profile,
                        primaryId: "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52"
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

        return NostrContentView(tree: tree)
            .padding()
    }
}

/// Article-reading surface: text selection → highlight, a range overlay, and a
/// footnote. Selecting body text surfaces a "Highlight" edit-menu action; the
/// "world" span is decorated; `[^1]` renders as a tappable footnote marker that
/// scrolls to its definition.
struct NostrContentArticlePreview: View {
    var body: some View {
        let tree = ContentTreeWire(
            nodes: [
                .text("Hello world — a body worth highlighting.[^1]"),
                .paragraph(children: [0]),
                .text("[^1]: A footnote definition rendered at the foot."),
                .paragraph(children: [2]),
            ],
            roots: [1, 3],
            mode: nil
        )
        return ScrollView {
            NostrContentView(
                tree: tree,
                decorations: [
                    NostrContentDecoration(id: "demo", quote: "world", color: .yellow.opacity(0.4))
                ],
                selectionEnabled: true
            )
            .padding()
        }
    }
}

#Preview {
    NostrContentViewPreview()
        .nostrContentRenderer(NostrContentRenderer())
}

#Preview("Article (selection / overlay / footnote)") {
    NostrContentArticlePreview()
        .nostrContentRenderer(
            NostrContentRenderer(
                callbacks: NostrContentCallbacks(
                    onTextSelected: { quote, _ in print("highlight:", quote) },
                    onDecorationTap: { id in print("tapped decoration:", id.raw) }
                )
            )
        )
}

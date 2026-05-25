import SwiftUI

/// Hand-built `ContentTreeWire` exercise for previews. Constructs the arena
/// directly rather than decoding JSON, so the preview doesn't need fixtures.
struct NostrContentViewPreview: View {
    var body: some View {
        // Arena layout:
        //   0  text "hello "
        //   1  mention(deadbeef…)
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
                        uri: "nostr:npub1example",
                        kind: .profile,
                        primaryId: "deadbeefcafebabedeadbeefcafebabe"
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

#Preview {
    NostrContentViewPreview()
        .nostrContentRenderer(NostrContentRenderer())
}

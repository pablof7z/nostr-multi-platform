import SwiftUI

struct NostrMinimalContentPreview: View {
    var body: some View {
        NostrMinimalContentView(
            runs: [
                NostrContentRun(id: "1", label: "hey ", kind: .text),
                NostrContentRun(id: "2", label: "@pablo", kind: .mention(pubkey: "npub1example")),
                NostrContentRun(id: "3", label: " check ", kind: .text),
                NostrContentRun(id: "4", label: "#nostr", kind: .hashtag("nostr")),
                NostrContentRun(
                    id: "5",
                    label: " nmp.dev",
                    kind: .link(URL(fileURLWithPath: "/"))
                ),
            ]
        )
        .padding()
    }
}

#Preview {
    NostrMinimalContentPreview()
}

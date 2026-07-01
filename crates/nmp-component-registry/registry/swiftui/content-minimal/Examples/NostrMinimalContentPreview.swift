import SwiftUI

struct NostrMinimalContentPreview: View {
    var body: some View {
        NostrMinimalContentView(
            runs: [
                NostrContentRun(id: "1", label: "hey ", kind: .text),
                NostrContentRun(
                    id: "2",
                    label: "@pablof7z",
                    kind: .mention(pubkey: "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52")
                ),
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

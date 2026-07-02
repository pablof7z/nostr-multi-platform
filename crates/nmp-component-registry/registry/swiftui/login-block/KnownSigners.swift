extension NostrSignerDetector {

    /// Ordered list of signers this detector knows about (detection precedence
    /// = array order). Every `urlScheme` here MUST also appear in Info.plist's
    /// `LSApplicationQueriesSchemes`.
    @MainActor
    public static let knownSigners: [NostrSignerInfo] = [
        NostrSignerInfo(kind: .amber, displayName: "Amber"),
        NostrSignerInfo(kind: .primal, displayName: "Primal"),
        NostrSignerInfo(
            kind: .generic(name: "Nostr Connect", scheme: "nostrconnect"),
            displayName: "Nostr Connect"
        ),
    ]
}

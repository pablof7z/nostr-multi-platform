import SwiftUI

public struct NostrContentCallbacks {
    public var onMentionTap: (String) -> Void
    public var onHashtagTap: (String) -> Void
    public var onLinkTap: (URL) -> Void

    public init(
        onMentionTap: @escaping (String) -> Void = { _ in },
        onHashtagTap: @escaping (String) -> Void = { _ in },
        onLinkTap: @escaping (URL) -> Void = { _ in }
    ) {
        self.onMentionTap = onMentionTap
        self.onHashtagTap = onHashtagTap
        self.onLinkTap = onLinkTap
    }
}

public struct NostrContentRenderer {
    public var textColor: Color
    public var mentionColor: Color
    public var hashtagColor: Color
    public var linkColor: Color
    public var callbacks: NostrContentCallbacks

    public init(
        textColor: Color = .primary,
        mentionColor: Color = .accentColor,
        hashtagColor: Color = .accentColor,
        linkColor: Color = .accentColor,
        callbacks: NostrContentCallbacks = NostrContentCallbacks()
    ) {
        self.textColor = textColor
        self.mentionColor = mentionColor
        self.hashtagColor = hashtagColor
        self.linkColor = linkColor
        self.callbacks = callbacks
    }
}

private struct NostrContentRendererKey: EnvironmentKey {
    static let defaultValue = NostrContentRenderer()
}

public extension EnvironmentValues {
    var nostrContentRenderer: NostrContentRenderer {
        get { self[NostrContentRendererKey.self] }
        set { self[NostrContentRendererKey.self] = newValue }
    }
}

public extension View {
    func nostrContentRenderer(_ renderer: NostrContentRenderer) -> some View {
        environment(\.nostrContentRenderer, renderer)
    }
}

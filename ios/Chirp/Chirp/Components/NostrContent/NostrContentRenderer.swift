import SwiftUI
import UIKit

public struct NostrContentCallbacks: @unchecked Sendable {
    public var onMentionTap: (String) -> Void
    public var onHashtagTap: (String) -> Void
    public var onLinkTap: (URL) -> Void
    public var onImageTap: (URL) -> Void
    public var onEventRefTap: (String) -> Void

    public init(
        onMentionTap: @escaping (String) -> Void = { _ in },
        onHashtagTap: @escaping (String) -> Void = { _ in },
        onLinkTap: @escaping (URL) -> Void = { _ in },
        onImageTap: ((URL) -> Void)? = nil,
        onEventRefTap: @escaping (String) -> Void = { _ in }
    ) {
        self.onMentionTap = onMentionTap
        self.onHashtagTap = onHashtagTap
        self.onLinkTap = onLinkTap
        // `onImageTap` defaults to `onLinkTap` so apps that only wire the
        // generic link handler still get image-tap routing for free.
        self.onImageTap = onImageTap ?? onLinkTap
        self.onEventRefTap = onEventRefTap
    }
}

public struct NostrContentRenderer: @unchecked Sendable {
    public var textColor: Color
    public var secondaryTextColor: Color
    public var mentionColor: Color
    public var hashtagColor: Color
    public var linkColor: Color
    public var quoteBorderColor: Color
    public var quoteBackgroundColor: Color
    public var codeBackgroundColor: Color
    public var placeholderColor: Color
    public var callbacks: NostrContentCallbacks
    public var emojiImages: [String: UIImage]

    public init(
        textColor: Color = .primary,
        secondaryTextColor: Color = .secondary,
        mentionColor: Color = .accentColor,
        hashtagColor: Color = .accentColor,
        linkColor: Color = .accentColor,
        quoteBorderColor: Color = Color.gray.opacity(0.35),
        quoteBackgroundColor: Color = Color.gray.opacity(0.08),
        codeBackgroundColor: Color = Color.gray.opacity(0.15),
        placeholderColor: Color = Color.gray.opacity(0.6),
        callbacks: NostrContentCallbacks = NostrContentCallbacks(),
        emojiImages: [String: UIImage] = [:]
    ) {
        self.textColor = textColor
        self.secondaryTextColor = secondaryTextColor
        self.mentionColor = mentionColor
        self.hashtagColor = hashtagColor
        self.linkColor = linkColor
        self.quoteBorderColor = quoteBorderColor
        self.quoteBackgroundColor = quoteBackgroundColor
        self.codeBackgroundColor = codeBackgroundColor
        self.placeholderColor = placeholderColor
        self.callbacks = callbacks
        self.emojiImages = emojiImages
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

import SwiftUI

/// Rich renderer for kind:0 profile metadata embeds.
///
/// Replaces `DefaultProfileRenderer` via `registry.setProfile(ProfileEmbed())`.
/// The projection is already resolved by Rust; this view only formats the raw
/// kind:0 fields and raw hex pubkey for display.
public struct ProfileEmbed: KindRenderer {
    public init() {}

    public func body(
        projection: EmbedKindProjection,
        registry: NostrKindRegistry
    ) -> AnyView {
        guard case .profile(let profile) = projection else {
            return AnyView(EmptyView())
        }

        let display = profile.displayName?.nonEmpty ?? shortHex(profile.pubkey)
        let picture = profile.pictureUrl.flatMap(URL.init(string:))

        return AnyView(
            VStack(alignment: .leading, spacing: 10) {
                HStack(alignment: .center, spacing: 10) {
                    NostrAvatar(
                        pubkey: profile.pubkey,
                        pictureUrl: picture,
                        size: 44,
                        consumerID: "content-kind-0.\(profile.pubkey)"
                    )

                    VStack(alignment: .leading, spacing: 3) {
                        Text(display)
                            .font(.headline)
                            .foregroundStyle(.primary)
                            .lineLimit(1)
                        if let nip05 = profile.nip05?.nonEmpty {
                            Label(nip05.stripRootNip05Prefix, systemImage: "checkmark.seal.fill")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                        }
                    }

                    Spacer(minLength: 0)
                    Text("kind:0")
                        .font(.caption2.monospaced())
                        .foregroundStyle(.tertiary)
                }

                Text(shortHex(profile.pubkey))
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)

                if let about = profile.about?.nonEmpty {
                    Text(about)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .lineLimit(4)
                }
            }
        )
    }

    private func shortHex(_ value: String) -> String {
        guard value.count > 16 else { return value }
        return "\(value.prefix(8))...\(value.suffix(8))"
    }
}

private extension String {
    var nonEmpty: String? {
        let trimmed = trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }

    var stripRootNip05Prefix: String {
        hasPrefix("_@") ? String(dropFirst(2)) : self
    }
}

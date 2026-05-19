import SwiftUI

struct Candidate: Identifiable, Equatable {
    enum Source { case follow, paste, manual }
    let pubkeyHex: String
    let source: Source
    var id: String { pubkeyHex }
}

struct ResolvedCandidate {
    enum Kind {
        case npub, nprofile, hex
        var label: String {
            switch self {
            case .npub: return "Pasted npub"
            case .nprofile: return "Pasted nprofile"
            case .hex: return "Pasted pubkey"
            }
        }
        var candidateSource: Candidate.Source { .paste }
    }
    let pubkeyHex: String
    let kind: Kind
}

// MARK: - Avatar

struct AvatarView: View {
    let profile: ProfileMetadata?
    let pubkeyHex: String
    let size: CGFloat

    var body: some View {
        let url = URL(string: profile?.picture ?? "")
        ZStack {
            if let url, let _ = url.scheme {
                KFImage(url)
                    .placeholder { fallback }
                    .resizable()
                    .scaledToFill()
            } else {
                fallback
            }
        }
        .frame(width: size, height: size)
        .clipShape(Circle())
        .overlay(
            Circle().stroke(Color.highlighterRule, lineWidth: 1)
        )
    }

    private var fallback: some View {
        ZStack {
            Color.highlighterTintPale
            Text(initial)
                .font(.system(size: size * 0.4, weight: .semibold, design: .default))
                .foregroundStyle(Color.highlighterInkStrong)
        }
    }

    private var initial: String {
        let name = profile?.name ?? ""
        if let first = name.first { return String(first).uppercased() }
        return String(pubkeyHex.prefix(1)).uppercased()
    }
}

// MARK: - Chip

struct Chip: View {
    let candidate: Candidate
    let profile: ProfileMetadata?
    let onRemove: () -> Void

    var body: some View {
        HStack(spacing: 8) {
            AvatarView(profile: profile, pubkeyHex: candidate.pubkeyHex, size: 22)
            Text(displayName)
                .font(.subheadline.weight(.medium))
                .foregroundStyle(Color.highlighterInkStrong)
                .lineLimit(1)
            Button(action: onRemove) {
                Image(systemName: "xmark")
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(Color.highlighterInkMuted)
            }
            .buttonStyle(.plain)
        }
        .padding(.leading, 6)
        .padding(.trailing, 10)
        .padding(.vertical, 5)
        .background(
            Capsule().fill(Color.highlighterTintPale)
        )
        .overlay(
            Capsule().stroke(Color.highlighterRule, lineWidth: 1)
        )
    }

    private var displayName: String {
        if let name = profile?.name, !name.isEmpty { return name }
        return String(candidate.pubkeyHex.prefix(8))
    }
}

// MARK: - Flow chips layout

struct FlowChips<Item: Identifiable, Content: View>: View {
    let items: [Item]
    @ViewBuilder let content: (Item) -> Content

    var body: some View {
        FlowLayout(spacing: 8) {
            ForEach(items) { item in
                content(item)
            }
        }
    }
}

private struct FlowLayout: Layout {
    var spacing: CGFloat = 8

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? .infinity
        var rowWidth: CGFloat = 0
        var totalHeight: CGFloat = 0
        var rowHeight: CGFloat = 0
        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if rowWidth + size.width > width {
                totalHeight += rowHeight + spacing
                rowWidth = size.width + spacing
                rowHeight = size.height
            } else {
                rowWidth += size.width + spacing
                rowHeight = max(rowHeight, size.height)
            }
        }
        return CGSize(width: width, height: totalHeight + rowHeight)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var x = bounds.minX
        var y = bounds.minY
        var rowHeight: CGFloat = 0
        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if x + size.width > bounds.maxX {
                x = bounds.minX
                y += rowHeight + spacing
                rowHeight = 0
            }
            subview.place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(size))
            x += size.width + spacing
            rowHeight = max(rowHeight, size.height)
        }
    }
}

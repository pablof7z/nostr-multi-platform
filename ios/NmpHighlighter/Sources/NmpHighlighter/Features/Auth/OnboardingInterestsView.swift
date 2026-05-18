import SwiftUI

struct OnboardingInterestsView: View {
    let account: GeneratedAccount

    @Environment(HighlighterStore.self) private var store

    @State private var selected: Set<String> = []
    @State private var isWorking = false

    private let interests = InterestCatalog.all

    var body: some View {
        ZStack {
            Color.highlighterPaper.ignoresSafeArea()

            VStack(alignment: .leading, spacing: 0) {
                VStack(alignment: .leading, spacing: 8) {
                    Text("What do you read?")
                        .font(.system(.title, design: .default).weight(.semibold))
                        .foregroundStyle(Color.highlighterInkStrong)

                    Text("Pick at least three — we'll pre-fill your feed with highlights from readers like you.")
                        .font(.callout)
                        .foregroundStyle(Color.highlighterInkMuted)
                        .lineSpacing(2)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .padding(.horizontal, 24)
                .padding(.top, 32)
                .padding(.bottom, 24)

                ScrollView {
                    chipGrid
                        .padding(.horizontal, 20)
                        .padding(.bottom, 120)
                }

                Spacer(minLength: 0)
            }

            VStack {
                Spacer()

                VStack(spacing: 8) {
                    if selected.count < 3 {
                        Text("Choose \(3 - selected.count) more")
                            .font(.caption)
                            .foregroundStyle(Color.highlighterInkMuted)
                            .transition(.opacity)
                    }

                    Button(action: finish) {
                        Group {
                            if isWorking {
                                ProgressView().tint(.white)
                            } else {
                                Text("Start exploring")
                                    .font(.headline)
                            }
                        }
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 14)
                    }
                    .buttonStyle(.glassProminent)
                    .disabled(selected.count < 3 || isWorking)
                    .padding(.horizontal, 32)
                    .animation(.easeInOut(duration: 0.15), value: selected.count)
                }
                .padding(.bottom, 48)
                .background(
                    LinearGradient(
                        colors: [Color.highlighterPaper.opacity(0), Color.highlighterPaper],
                        startPoint: .top,
                        endPoint: UnitPoint(x: 0.5, y: 0.6)
                    )
                    .ignoresSafeArea()
                )
            }
        }
        .navigationBarBackButtonHidden(true)
        .animation(.easeInOut(duration: 0.1), value: selected)
    }

    private var chipGrid: some View {
        FlowLayout(spacing: 10) {
            ForEach(interests, id: \.id) { interest in
                chip(interest)
            }
        }
    }

    private func chip(_ interest: InterestCatalog.Interest) -> some View {
        let active = selected.contains(interest.id)
        return Button {
            if active {
                selected.remove(interest.id)
            } else {
                selected.insert(interest.id)
            }
        } label: {
            HStack(spacing: 6) {
                Text(interest.emoji)
                    .font(.body)
                Text(interest.label)
                    .font(.subheadline.weight(active ? .semibold : .regular))
                    .foregroundStyle(active ? Color.white : Color.highlighterInkStrong)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 9)
            .background(active ? Color.highlighterAccent : Color.highlighterInkStrong.opacity(0.08),
                        in: .capsule)
        }
        .buttonStyle(.plain)
    }

    private func finish() {
        guard !isWorking else { return }
        isWorking = true
        let chosenIds = selected

        Task {
            await store.completeLogin(user: account.user)
            UserDefaults.standard.set(true, forKey: "onboardingComplete")

            let pubkeys = InterestCatalog.pubkeys(for: chosenIds)
            for pubkey in pubkeys {
                try? await store.safeCore.setFollow(targetPubkeyHex: pubkey, follow: true)
            }
        }
    }
}

// MARK: - Interest catalog

enum InterestCatalog {
    struct Interest: Identifiable {
        let id: String
        let emoji: String
        let label: String
    }

    static let all: [Interest] = [
        Interest(id: "philosophy", emoji: "🧠", label: "Philosophy"),
        Interest(id: "science_fiction", emoji: "🚀", label: "Science Fiction"),
        Interest(id: "technology", emoji: "💻", label: "Technology"),
        Interest(id: "history", emoji: "📜", label: "History"),
        Interest(id: "economics", emoji: "📈", label: "Economics"),
        Interest(id: "psychology", emoji: "🔬", label: "Psychology"),
        Interest(id: "literature", emoji: "📚", label: "Literature"),
        Interest(id: "politics", emoji: "🗳️", label: "Politics"),
        Interest(id: "bitcoin", emoji: "₿", label: "Bitcoin"),
        Interest(id: "self_improvement", emoji: "🌱", label: "Self-improvement"),
        Interest(id: "science", emoji: "🔭", label: "Science"),
        Interest(id: "art", emoji: "🎨", label: "Art"),
        Interest(id: "music", emoji: "🎵", label: "Music"),
        Interest(id: "design", emoji: "✏️", label: "Design"),
        Interest(id: "writing", emoji: "✍️", label: "Writing"),
        Interest(id: "startups", emoji: "⚡️", label: "Startups"),
        Interest(id: "nostr", emoji: "🟣", label: "Nostr"),
        Interest(id: "food", emoji: "🍳", label: "Food"),
        Interest(id: "travel", emoji: "🗺️", label: "Travel"),
        Interest(id: "health", emoji: "🏃", label: "Health"),
    ]

    /// Returns a deduplicated list of pubkeys to follow for the given interest ids.
    static func pubkeys(for ids: Set<String>) -> [String] {
        var result: [String] = []
        var seen = Set<String>()
        for id in ids {
            for pk in curatedPubkeys[id] ?? [] {
                if seen.insert(pk).inserted {
                    result.append(pk)
                }
            }
        }
        return result
    }

    // Curated Nostr pubkeys (hex) per interest, sourced from public Nostr profiles.
    private static let curatedPubkeys: [String: [String]] = [
        "philosophy": [
            "82341f882b6eabcd2ba7f1ef90aad961cf074af15b9ef44a09f9d2a8fbfbe6a2", // jack
        ],
        "technology": [
            "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d", // fiatjaf
            "82341f882b6eabcd2ba7f1ef90aad961cf074af15b9ef44a09f9d2a8fbfbe6a2", // jack
        ],
        "bitcoin": [
            "82341f882b6eabcd2ba7f1ef90aad961cf074af15b9ef44a09f9d2a8fbfbe6a2", // jack
            "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d", // fiatjaf
        ],
        "nostr": [
            "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d", // fiatjaf
        ],
        "writing": [
            "82341f882b6eabcd2ba7f1ef90aad961cf074af15b9ef44a09f9d2a8fbfbe6a2",
        ],
        "science_fiction": [
            "82341f882b6eabcd2ba7f1ef90aad961cf074af15b9ef44a09f9d2a8fbfbe6a2",
        ],
        "history": [
            "82341f882b6eabcd2ba7f1ef90aad961cf074af15b9ef44a09f9d2a8fbfbe6a2",
        ],
        "economics": [
            "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d",
        ],
        "psychology": [
            "82341f882b6eabcd2ba7f1ef90aad961cf074af15b9ef44a09f9d2a8fbfbe6a2",
        ],
        "literature": [
            "82341f882b6eabcd2ba7f1ef90aad961cf074af15b9ef44a09f9d2a8fbfbe6a2",
        ],
        "startups": [
            "82341f882b6eabcd2ba7f1ef90aad961cf074af15b9ef44a09f9d2a8fbfbe6a2",
        ],
        "self_improvement": [
            "82341f882b6eabcd2ba7f1ef90aad961cf074af15b9ef44a09f9d2a8fbfbe6a2",
        ],
        "politics": [],
        "science": [],
        "art": [],
        "music": [],
        "design": [],
        "food": [],
        "travel": [],
        "health": [],
    ]
}

// MARK: - FlowLayout

private struct FlowLayout: Layout {
    var spacing: CGFloat = 8

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let rows = computeRows(proposal: proposal, subviews: subviews)
        let height = rows.map(\.height).reduce(0, +) + CGFloat(max(rows.count - 1, 0)) * spacing
        return CGSize(width: proposal.width ?? 0, height: height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let rows = computeRows(proposal: ProposedViewSize(width: bounds.width, height: nil), subviews: subviews)
        var y = bounds.minY
        for row in rows {
            var x = bounds.minX
            for item in row.items {
                item.view.place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(item.size))
                x += item.size.width + spacing
            }
            y += row.height + spacing
        }
    }

    private struct Row {
        var items: [(view: LayoutSubview, size: CGSize)] = []
        var height: CGFloat = 0
    }

    private func computeRows(proposal: ProposedViewSize, subviews: Subviews) -> [Row] {
        let maxWidth = proposal.width ?? .infinity
        var rows: [Row] = []
        var current = Row()
        var currentWidth: CGFloat = 0

        for view in subviews {
            let size = view.sizeThatFits(ProposedViewSize(width: nil, height: nil))
            if currentWidth + size.width > maxWidth, !current.items.isEmpty {
                rows.append(current)
                current = Row()
                currentWidth = 0
            }
            current.items.append((view, size))
            current.height = max(current.height, size.height)
            currentWidth += size.width + spacing
        }
        if !current.items.isEmpty { rows.append(current) }
        return rows
    }
}

import SwiftUI

/// Free-text NIP-50 search over the higher-order `open_search` C-ABI.
///
/// THIN SHELL: this view contains ZERO search logic. It owns only presentation
/// state (the query text + scope toggle) and delegates everything else to
/// `SearchController`, which (1) shapes the `SearchRequest` JSON, (2) calls the
/// `KernelHandle` C-ABI passthroughs (`openSearch` / `closeSearch` / the
/// `nmp_app_search_snapshot` pull), and (3) decodes the typed `N50S` FlatBuffers
/// sidecar via `SearchResultsDecoder`.
///
/// Query validation, relay selection (UserPreferred → the active account's
/// kind:10007 list, wired transitively by `register_defaults`), the local
/// cache-FTS scan, dedup, and result ordering all live in NMP/Rust. Results
/// include both local cache hits and relay hits.
///
/// The controller owns one search session id for its lifetime; submitting a new
/// query re-opens search under the SAME session id (the kernel replaces the
/// prior plan), and `.onDisappear` closes it.
struct SearchSheet: View {
    @EnvironmentObject private var model: KernelModel
    @Environment(\.dismiss) private var dismiss
    @StateObject private var controller = SearchController()

    @State private var query = ""
    @State private var scope: SearchScope = .people

    /// The result-scope toggle. `People` maps to NIP-50 `scope: "Users"`
    /// (kind:0 profiles); `Notes` maps to `scope: {"Kinds":[1]}` (kind:1 notes).
    private enum SearchScope: String, CaseIterable, Identifiable {
        case people = "People"
        case notes = "Notes"
        var id: String { rawValue }

        /// The serde value the Rust `SearchScope` enum accepts: a bare string
        /// `"Users"`, or the externally-tagged `{"Kinds":[1]}` object.
        var jsonValue: Any {
            switch self {
            case .people: return "Users"
            case .notes: return ["Kinds": [1]]
            }
        }
    }

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                scopePicker
                resultsList
            }
            .background(ChirpColor.bg.ignoresSafeArea())
            .navigationTitle("Search")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                }
            }
            .searchable(
                text: $query,
                placement: .navigationBarDrawer(displayMode: .always),
                prompt: "Search Nostr"
            )
            .onSubmit(of: .search) { runSearch() }
            .onChange(of: scope) { if controller.submittedQuery != nil { runSearch() } }
            .onAppear { controller.bind(to: model) }
            .onDisappear { controller.close() }
        }
    }

    // ── Scope toggle ──────────────────────────────────────────────────────────

    private var scopePicker: some View {
        Picker("Scope", selection: $scope) {
            ForEach(SearchScope.allCases) { Text($0.rawValue).tag($0) }
        }
        .pickerStyle(.segmented)
        .padding(.horizontal, ChirpSpace.l)
        .padding(.vertical, ChirpSpace.s)
    }

    // ── Results ───────────────────────────────────────────────────────────────

    @ViewBuilder
    private var resultsList: some View {
        if let submitted = controller.submittedQuery {
            if controller.hits.isEmpty {
                placeholder(
                    systemImage: "questionmark.circle",
                    title: "No results",
                    subtitle: "Nothing matched \u{201C}\(submitted)\u{201D} yet."
                )
            } else {
                List(controller.hits) { hit in
                    SearchHitRow(hit: hit)
                        .listRowBackground(ChirpColor.bg)
                        .listRowSeparatorTint(ChirpColor.hairlineSoft)
                }
                .listStyle(.plain)
                .scrollContentBackground(.hidden)
            }
        } else {
            placeholder(
                systemImage: "magnifyingglass",
                title: "Search Nostr",
                subtitle: "Find people and notes across your relays and local cache."
            )
        }
    }

    private func placeholder(systemImage: String, title: String, subtitle: String) -> some View {
        ChirpPlaceholder(systemImage: systemImage, title: title, subtitle: subtitle)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(.horizontal, ChirpSpace.l)
    }

    // ── Dispatch ──────────────────────────────────────────────────────────────

    private func runSearch() {
        controller.runSearch(query: query, scopeJSON: scope.jsonValue)
    }
}

/// One NIP-50 search result row. Renders a profile hit (kind:0) as an avatar +
/// name, and a note hit (kind:1 / long-form) as the author avatar + content
/// snippet. The avatar resolves the author's `refs.profile` row per-key on its
/// own (D4), so this row passes only the raw pubkey.
private struct SearchHitRow: View {
    let hit: ChirpSearchHit

    private var isProfile: Bool { hit.kind == 0 }

    var body: some View {
        HStack(alignment: .top, spacing: ChirpSpace.m) {
            NostrAvatar(
                pubkey: hit.author,
                initials: hit.author.displayInitials,
                colorHex: hit.author.pubkeyColorHex,
                size: 40
            )
            .equatable()

            VStack(alignment: .leading, spacing: ChirpSpace.xs) {
                NostrProfileName(
                    pubkey: hit.author,
                    font: ChirpFont.headline,
                    color: ChirpColor.textPrimary
                )

                if !snippet.isEmpty {
                    Text(snippet)
                        .font(ChirpFont.callout)
                        .foregroundStyle(ChirpColor.textSecondary)
                        .lineLimit(isProfile ? 2 : 4)
                }

                provenanceBadge
            }
            Spacer(minLength: 0)
        }
        .padding(.vertical, ChirpSpace.xs)
    }

    /// Note hits show the raw content; profile hits show the author key short
    /// form (the profile JSON in `content` is rendered as a name by the avatar
    /// + display-name views above, so the raw kind:0 JSON is not shown).
    private var snippet: String {
        if isProfile {
            return hit.author.shortHex
        }
        return hit.content.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private var provenanceBadge: some View {
        HStack(spacing: ChirpSpace.xs) {
            Image(systemName: hit.isCache ? "internaldrive" : "antenna.radiowaves.left.and.right")
                .font(.system(size: 10, weight: .semibold))
            Text(hit.isCache ? "Cache" : (hit.sourceRelay.isEmpty ? "Relay" : hit.sourceRelay))
                .font(ChirpFont.caption)
                .lineLimit(1)
        }
        .foregroundStyle(ChirpColor.textTertiary)
    }
}

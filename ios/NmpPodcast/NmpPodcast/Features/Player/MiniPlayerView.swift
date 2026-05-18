import SwiftUI

/// Persistent mini-player presented as a `tabViewBottomAccessory` (iOS 26).
///
/// Reads `\.tabViewBottomAccessoryPlacement` from the environment and
/// renders one of two layouts:
///   - `.expanded` — full mini-bar above the tab bar with the episode title.
///   - `.inline`   — compact pill that slots between the active-tab capsule
///     and the trailing toolbar controls when the tab bar collapses on
///     scroll-down (Apple Music pattern).
///
/// The expanded UI shows artwork, the episode title, the show name + clock,
/// and play / +30s. The inline pill drops to artwork + play/pause only.
struct MiniPlayerView: View {

    @Environment(AppStateStore.self) private var store
    @Bindable var state: PlaybackState
    let onTap: () -> Void
    let glassNamespace: Namespace.ID

    @Environment(\.tabViewBottomAccessoryPlacement) private var placement

    /// Observed so the inline download badge tracks the service's
    /// `progress[id]` map without each 5%/200ms tick re-rendering through
    /// `AppStateStore`. Mirrors the pattern used by `EpisodeRow`.
    @State private var downloadService = EpisodeDownloadService.shared

    private var showName: String {
        guard let subID = state.episode?.podcastID,
              let sub = store.podcast(id: subID) else { return "" }
        return sub.title
    }

    /// Title of the chapter containing the playhead, when the live episode
    /// has navigable chapters. Returns `nil` for chapter-less episodes so
    /// the metadata line falls back to the show name. Reads from
    /// `AppStateStore` rather than the cached `state.episode` so chapters
    /// hydrated by `ChaptersHydrationService` after playback started show
    /// up here without a re-load.
    private var activeChapterTitle: String? {
        guard let stateEpisode = state.episode else { return nil }
        let live = store.episode(id: stateEpisode.id) ?? stateEpisode
        let navigable = live.chapters?.filter(\.includeInTableOfContents) ?? []
        guard !navigable.isEmpty else { return nil }
        return navigable.active(at: state.currentTime)?.title
    }

    var body: some View {
        Group {
            switch placement {
            case .inline:
                inlineBody
            default:
                expandedBody
            }
        }
        .animation(AppTheme.Animation.spring, value: placement)
    }

    // MARK: - Expanded (regular) layout

    private var expandedBody: some View {
        // Wrapping in `Button(action: onTap)` collapses every nested Button
        // (Pause, Skip Forward) into the parent's tap target — sighted users
        // tapping the visible pause icon end up *expanding* the player
        // instead of pausing. Use a non-Button tap surface so the nested
        // transport Buttons keep their own gestures.
        content
            .glassEffect(.regular.interactive(), in: .rect(cornerRadius: AppTheme.Corner.lg))
            .glassEffectID("player.surface", in: glassNamespace)
            .contentShape(.rect(cornerRadius: AppTheme.Corner.lg))
            .onTapGesture {
                Haptics.light()
                onTap()
            }
            .accessibilityElement(children: .contain)
            .accessibilityAction(named: "Open player") {
                onTap()
            }
    }

    // MARK: - Inline (compact) layout

    /// The collapsed pill that sits inline with the tab bar. No surrounding
    /// glass surface — the toolbar's own glass shell hosts it.
    ///
    /// Same Button-inside-Button trap as `expandedBody`: the play/pause icon
    /// has to remain a real, separately-tappable Button. Use a non-Button
    /// tap surface for the expand-on-tap action.
    ///
    /// Title is included alongside the artwork — without it, the pill reads
    /// as a generic glass slab and the underlying scroll content shows
    /// through the translucent background, making it look broken. Apple
    /// Music's pill omits the title because their artwork conveys identity
    /// strongly; podcast covers don't, so we need text.
    private var inlineBody: some View {
        HStack(spacing: AppTheme.Spacing.xs) {
            // Combine artwork + title + clock into a single tap-to-expand
            // surface. Each child is `accessibilityHidden` so VoiceOver
            // hears one labeled "Now Playing" element, not three. The
            // tap-to-expand was previously unreachable for VO users.
            HStack(spacing: AppTheme.Spacing.xs) {
                inlineArtwork
                    .glassEffectID("player.artwork", in: glassNamespace)
                    .accessibilityHidden(true)

                VStack(alignment: .leading, spacing: 0) {
                    inlineTitle
                    inlineClock
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .accessibilityHidden(true)
            }
            .contentShape(Rectangle())
            .onTapGesture {
                Haptics.light()
                onTap()
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(accessibilityLabel)
            .accessibilityHint("Opens the full player")
            .accessibilityAddTraits(.isButton)

            inlineDownloadBadge

            // Real Button kept as a sibling so its 44pt hit area never
            // gets eaten by the expand-tap surface. `.frame(28)` keeps the
            // visible glyph compact; the outer `.frame(44)` + .contentShape
            // expands the actual tap target to Apple's HIG minimum.
            Button {
                state.togglePlayPause()
            } label: {
                Image(systemName: state.isPlaying ? "pause.fill" : "play.fill")
                    .font(.subheadline.weight(.bold))
                    .foregroundStyle(.primary)
                    .frame(width: 28, height: 28)
                    .frame(width: 44, height: 44)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.pressable)
            .accessibilityLabel(state.isPlaying ? "Pause" : "Play")
        }
        .padding(.horizontal, AppTheme.Spacing.sm)
    }

    /// Re-resolves `state.episode` through the store so coarse download
    /// transitions (`.downloading → .downloaded`) reach the badge without
    /// requiring `PlaybackState` to refresh its cached snapshot.
    private var liveDownloadEpisode: Episode? {
        guard let id = state.episode?.id else { return nil }
        return store.episode(id: id) ?? state.episode
    }

    @ViewBuilder
    private var inlineTitle: some View {
        if let episode = state.episode {
            Text(episode.title)
                .font(.footnote.weight(.semibold))
                .foregroundStyle(.primary)
                .lineLimit(1)
                .truncationMode(.tail)
        }
    }

    /// Compact mono-digit playhead surfaced inline so the collapsed pill
    /// keeps a glanceable cue without the full metadata line. Hidden when
    /// no episode is loaded (the artwork+title row already conveys "no
    /// playback") to avoid an empty 0:00 leaking into the layout.
    @ViewBuilder
    private var inlineClock: some View {
        if state.episode != nil {
            Text(PlayerTimeFormat.clock(state.currentTime))
                .font(.system(size: 11, weight: .regular, design: .monospaced))
                .foregroundStyle(.secondary)
                .monospacedDigit()
                .lineLimit(1)
        }
    }

    /// Inline-only download surface — narrow visibility rule per spec:
    /// only render when the live episode is actively downloading or has
    /// failed. The collapsed pill has no horizontal slack for `.queued`
    /// or terminal `.downloaded` states, and they'd add visual noise to
    /// the tab bar without informing an in-flight action.
    @ViewBuilder
    private var inlineDownloadBadge: some View {
        if let resolved = liveDownloadEpisode {
            switch resolved.downloadState {
            case .downloading, .failed:
                DownloadProgressBadge(
                    episode: resolved,
                    liveProgress: downloadService.progress[resolved.id]
                )
            default:
                EmptyView()
            }
        }
    }

    private var inlineArtwork: some View {
        artworkSurface(
            size: 26,
            cornerRadius: AppTheme.Corner.sm,
            placeholderGlyphSize: 10
        )
    }

    // MARK: - Subviews

    private var content: some View {
        HStack(spacing: AppTheme.Spacing.sm) {
            artwork
                .glassEffectID("player.artwork", in: glassNamespace)

            VStack(alignment: .leading, spacing: 2) {
                titleLine
                metadataLine
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            transportButtons
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 14)
    }

    private var artwork: some View {
        artworkSurface(
            size: 42,
            cornerRadius: AppTheme.Corner.md,
            placeholderGlyphSize: 17
        )
    }

    /// Resolved artwork URL — episode override first, then the show-level
    /// fallback via `PlaybackState.resolveShowImage` (the same closure the
    /// full Player uses, wired in `RootView`).
    private var artworkURL: URL? {
        guard let episode = state.episode else { return nil }
        return episode.imageURL ?? state.resolveShowImage(episode)
    }

    /// Shared artwork rendering for both the expanded (44pt) and inline
    /// (26pt) layouts. Loading state is glyph-free so the user doesn't read
    /// it as "no artwork"; failure state shows a subtle waveform indicator.
    @ViewBuilder
    private func artworkSurface(
        size: CGFloat,
        cornerRadius: CGFloat,
        placeholderGlyphSize: CGFloat
    ) -> some View {
        ZStack {
            if let url = artworkURL {
                CachedAsyncImage(url: url, targetSize: CGSize(width: 64, height: 64)) { phase in
                    switch phase {
                    case .success(let image):
                        image
                            .resizable()
                            .scaledToFill()
                    case .failure:
                        miniArtworkFailureFallback(glyphSize: placeholderGlyphSize)
                    case .empty:
                        Color.secondary.opacity(0.18)
                    @unknown default:
                        Color.secondary.opacity(0.18)
                    }
                }
            } else {
                miniArtworkFailureFallback(glyphSize: placeholderGlyphSize)
            }
        }
        .frame(width: size, height: size)
        .clipShape(RoundedRectangle(cornerRadius: cornerRadius, style: .continuous))
    }

    private func miniArtworkFailureFallback(glyphSize: CGFloat) -> some View {
        ZStack {
            Color.secondary.opacity(0.18)
            Image(systemName: "waveform")
                .font(.system(size: glyphSize, weight: .semibold))
                .foregroundStyle(.secondary)
        }
    }

    @ViewBuilder
    private var titleLine: some View {
        if let episode = state.episode {
            Text(episode.title)
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(.primary)
                .lineLimit(1)
                .truncationMode(.tail)
        }
    }

    @ViewBuilder
    private var metadataLine: some View {
        if state.episode != nil {
            HStack(spacing: 6) {
                if let chapterTitle = activeChapterTitle {
                    // `list.bullet.rectangle` reads as "chapter / item in
                    // a list" — the previous `book.pages` glyph reads as
                    // "show notes / read more" and conflicted with the
                    // long-form notes affordance.
                    Image(systemName: "list.bullet.rectangle")
                        .font(.system(size: 9, weight: .semibold))
                        .foregroundStyle(.tint)
                        .accessibilityHidden(true)
                    Text(chapterTitle)
                        .font(AppTheme.Typography.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                        .transition(.opacity)
                        .id(chapterTitle)
                    Text("·")
                        .font(AppTheme.Typography.caption)
                        .foregroundStyle(.tertiary)
                        .accessibilityHidden(true)
                } else if !showName.isEmpty {
                    Text(showName)
                        .font(AppTheme.Typography.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                    Text("·")
                        .font(AppTheme.Typography.caption)
                        .foregroundStyle(.tertiary)
                        .accessibilityHidden(true)
                }
                Text(PlayerTimeFormat.clock(state.currentTime))
                    .font(AppTheme.Typography.monoCaption)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
            }
            .animation(AppTheme.Animation.spring, value: activeChapterTitle)
        }
    }

    private var transportButtons: some View {
        HStack(spacing: AppTheme.Spacing.xs) {
            Button {
                state.togglePlayPause()
            } label: {
                Image(systemName: state.isPlaying ? "pause.fill" : "play.fill")
                    .font(.title3.weight(.bold))
                    .foregroundStyle(.primary)
                    .frame(width: 36, height: 36)
                    .frame(width: 44, height: 44)
                    .contentShape(Rectangle())
                    .glassEffectID("player.play", in: glassNamespace)
            }
            .buttonStyle(.pressable)
            .accessibilityLabel(state.isPlaying ? "Pause" : "Play")

            Button {
                state.skipForward()
            } label: {
                Image(systemName: forwardSkipGlyph)
                    .font(.title3.weight(.semibold))
                    .foregroundStyle(.primary)
                    .frame(width: 36, height: 36)
                    .frame(width: 44, height: 44)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.pressable)
            .accessibilityLabel("Skip forward \(state.skipForwardSeconds) seconds")

            Button {
                dismissCurrentEpisode()
            } label: {
                Image(systemName: "xmark")
                    .font(.callout.weight(.semibold))
                    .foregroundStyle(.secondary)
                    .frame(width: 36, height: 36)
                    .frame(width: 44, height: 44)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.pressable)
            .accessibilityLabel("Dismiss")
        }
    }

    private func dismissCurrentEpisode() {
        guard let episodeID = state.episode?.id else { return }
        Haptics.warning()
        state.pause()
        state.episode = nil
        store.markEpisodePlayed(episodeID)
    }

    private var accessibilityLabel: String {
        let title = state.episode?.title ?? "Now playing"
        var parts: [String] = [title]
        if let chapter = activeChapterTitle {
            parts.append("Chapter: \(chapter)")
        } else if !showName.isEmpty {
            parts.append(showName)
        }
        return parts.joined(separator: ", ")
    }

    /// Picks the SF Symbol that *exactly* matches the user's configured
    /// skip-forward interval. iOS only ships numeric variants for
    /// `{10, 15, 30, 45, 60, 75, 90}`; anything off-grid falls back to bare
    /// `goforward` (no number) so the visible label never lies about the
    /// actual skip seconds — a 20 s skip used to render `goforward.15`.
    private var forwardSkipGlyph: String {
        let supported = [10, 15, 30, 45, 60, 75, 90]
        let seconds = state.skipForwardSeconds
        return supported.contains(seconds) ? "goforward.\(seconds)" : "goforward"
    }
}

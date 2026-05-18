import SwiftUI
import os.log

// MARK: - OnboardingSubscribePage
//
// Optional onboarding step inserted between identity setup and the final
// "ready" page. Lets a brand-new user pick their first podcast right away —
// either by browsing a curated list of starters or by pasting an RSS feed
// URL — so the Library tab isn't empty on first launch.
//
// Skipping is fine; the page exists to remove a potential dead-end, not to
// require an action.

struct OnboardingSubscribePage: View {

    nonisolated private static let logger = Logger.app("OnboardingSubscribe")

    /// Closure invoked once a subscription has been added so the parent can
    /// react (e.g. flip a "has subscribed" flag that re-labels the primary
    /// button). Failures stay inline; the page does NOT auto-navigate on
    /// success — the user needs to see the row flip to a checkmark first,
    /// otherwise the tap reads as a no-op.
    var onSubscribed: (Podcast) -> Void

    /// Curated, vendor-agnostic starter shows. Every URL points at a public
    /// RSS feed that has reliably hosted episodes for years; the goal is to
    /// give first-run users a few well-known options without forcing a
    /// network search.
    private static let suggestions: [Suggestion] = [
        Suggestion(
            title: "The Daily",
            author: "The New York Times",
            feed: "https://feeds.simplecast.com/54nAGcIl",
            tint: .red
        ),
        Suggestion(
            title: "Hard Fork",
            author: "The New York Times",
            feed: "https://feeds.simplecast.com/l2i9YnTd",
            tint: .orange
        ),
        Suggestion(
            title: "Lex Fridman Podcast",
            author: "Lex Fridman",
            feed: "https://lexfridman.com/feed/podcast/",
            tint: .indigo
        ),
        Suggestion(
            title: "Acquired",
            author: "Ben Gilbert & David Rosenthal",
            feed: "https://feeds.transistor.fm/acquired",
            tint: .teal
        ),
    ]

    @Environment(AppStateStore.self) private var store

    @State private var feedURL: String = ""
    @State private var isWorking: Bool = false
    @State private var errorMessage: String?
    /// ID of the suggestion row currently being subscribed to, for spinner state.
    @State private var subscribingSuggestionID: UUID?

    var body: some View {
        VStack(spacing: AppTheme.Spacing.lg) {
            Spacer(minLength: 0)
            pageIcon
            pageHeader
            suggestionsList
            urlEntry
            errorView
            Spacer(minLength: 0)
        }
        .animation(AppTheme.Animation.springFast, value: errorMessage)
        .animation(AppTheme.Animation.springFast, value: isWorking)
    }

    // MARK: - Subviews

    private var pageIcon: some View {
        Image(systemName: "antenna.radiowaves.left.and.right")
            .font(.system(size: OnboardingLayout.pageIconSize, weight: .semibold))
            .foregroundStyle(.white)
            .symbolEffect(.pulse, options: .repeating)
            .padding(OnboardingLayout.pageIconPadding)
            .glassEffect(.regular, in: .circle)
            .overlay(Circle().strokeBorder(.white.opacity(OnboardingLayout.pageIconStroke), lineWidth: 1))
    }

    private var pageHeader: some View {
        VStack(spacing: AppTheme.Spacing.sm) {
            Text("Add your first show")
                .font(AppTheme.Typography.largeTitle)
                .foregroundStyle(.white)
            Text("Pick a popular podcast or paste an RSS feed URL. You can add more later.")
                .font(AppTheme.Typography.body)
                .foregroundStyle(.white.opacity(0.8))
                .multilineTextAlignment(.center)
                .padding(.horizontal, AppTheme.Spacing.md)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private var suggestionsList: some View {
        VStack(spacing: AppTheme.Spacing.xs) {
            ForEach(Self.suggestions) { suggestion in
                suggestionRow(suggestion)
            }
        }
        .padding(.horizontal, AppTheme.Spacing.sm)
    }

    private func suggestionRow(_ suggestion: Suggestion) -> some View {
        Button {
            Task { await subscribe(to: suggestion) }
        } label: {
            HStack(spacing: AppTheme.Spacing.sm) {
                Image(systemName: "headphones")
                    .font(.system(size: 18, weight: .semibold))
                    .foregroundStyle(.white)
                    .frame(width: 32, height: 32)
                    .background(suggestion.tint.opacity(0.85), in: RoundedRectangle(cornerRadius: 7, style: .continuous))
                VStack(alignment: .leading, spacing: 1) {
                    Text(suggestion.title)
                        .font(AppTheme.Typography.body.weight(.semibold))
                        .foregroundStyle(.white)
                        .lineLimit(1)
                    Text(suggestion.author)
                        .font(AppTheme.Typography.caption)
                        .foregroundStyle(.white.opacity(0.7))
                        .lineLimit(1)
                }
                Spacer(minLength: 0)
                if subscribingSuggestionID == suggestion.id {
                    ProgressView().tint(.white)
                } else if isAlreadySubscribed(to: suggestion) {
                    Image(systemName: "checkmark.circle.fill")
                        .foregroundStyle(.green)
                } else {
                    Image(systemName: "plus.circle.fill")
                        .foregroundStyle(.white.opacity(0.85))
                }
            }
            .padding(.horizontal, AppTheme.Spacing.md)
            .padding(.vertical, OnboardingLayout.fieldVerticalPadding - 2)
            .glassEffect(.regular, in: .rect(cornerRadius: AppTheme.Corner.lg))
            .overlay(
                RoundedRectangle(cornerRadius: AppTheme.Corner.lg, style: .continuous)
                    .strokeBorder(.white.opacity(0.20), lineWidth: 1)
            )
        }
        .buttonStyle(.plain)
        .disabled(isWorking || isAlreadySubscribed(to: suggestion))
    }

    private var urlEntry: some View {
        VStack(spacing: AppTheme.Spacing.xs) {
            HStack(spacing: AppTheme.Spacing.sm) {
                Image(systemName: "link")
                    .foregroundStyle(.white.opacity(0.7))
                TextField("Paste an RSS feed URL", text: $feedURL)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .keyboardType(.URL)
                    .submitLabel(.go)
                    .foregroundStyle(.white)
                    .onSubmit { Task { await subscribeToTypedURL() } }
                Button {
                    Task { await subscribeToTypedURL() }
                } label: {
                    if isWorking, subscribingSuggestionID == nil {
                        ProgressView().tint(.white)
                    } else {
                        Image(systemName: "arrow.right.circle.fill")
                            .foregroundStyle(.white.opacity(feedURL.isBlank ? 0.4 : 0.95))
                    }
                }
                .buttonStyle(.plain)
                .disabled(feedURL.isBlank || isWorking)
            }
            .padding(.horizontal, AppTheme.Spacing.md)
            .padding(.vertical, OnboardingLayout.fieldVerticalPadding)
            .glassEffect(.regular, in: .rect(cornerRadius: AppTheme.Corner.lg))
            .overlay(
                RoundedRectangle(cornerRadius: AppTheme.Corner.lg, style: .continuous)
                    .strokeBorder(.white.opacity(0.25), lineWidth: 1)
            )
        }
        .padding(.horizontal, AppTheme.Spacing.sm)
    }

    @ViewBuilder
    private var errorView: some View {
        if let errorMessage {
            Text(errorMessage)
                .font(AppTheme.Typography.caption)
                .foregroundStyle(AppTheme.Tint.errorOnDark)
                .multilineTextAlignment(.center)
                .padding(.horizontal, AppTheme.Spacing.md)
                .transition(.opacity)
        }
    }

    // MARK: - Actions

    private func subscribe(to suggestion: Suggestion) async {
        guard !isWorking else { return }
        errorMessage = nil
        isWorking = true
        subscribingSuggestionID = suggestion.id
        defer {
            isWorking = false
            subscribingSuggestionID = nil
        }
        let service = SubscriptionService(store: store)
        do {
            let added = try await service.addSubscription(feedURLString: suggestion.feed)
            Haptics.success()
            // Sanity check the subscription is actually persisted — if a
            // future regression starts losing writes, we'd rather log than
            // silently flip the checkmark.
            if let url = URL(string: suggestion.feed),
               store.podcast(feedURL: url) == nil {
                Self.logger.error(
                    "Subscription \(suggestion.title, privacy: .public) reported success but was not found in store after add"
                )
            }
            onSubscribed(added)
        } catch let addError as SubscriptionService.AddError {
            Self.logger.error(
                "Failed to subscribe to \(suggestion.title, privacy: .public) (\(suggestion.feed, privacy: .public)): \(addError.localizedDescription, privacy: .public)"
            )
            errorMessage = addError.localizedDescription
            Haptics.warning()
        } catch {
            Self.logger.error(
                "Unexpected error subscribing to \(suggestion.title, privacy: .public): \(error.localizedDescription, privacy: .public)"
            )
            errorMessage = error.localizedDescription
            Haptics.warning()
        }
    }

    private func subscribeToTypedURL() async {
        let trimmed = feedURL.trimmed
        guard !trimmed.isEmpty, !isWorking else { return }
        errorMessage = nil
        isWorking = true
        defer { isWorking = false }
        let service = SubscriptionService(store: store)
        do {
            let added = try await service.addSubscription(feedURLString: trimmed)
            Haptics.success()
            feedURL = ""
            onSubscribed(added)
        } catch let addError as SubscriptionService.AddError {
            Self.logger.error(
                "Failed to subscribe to typed URL \(trimmed, privacy: .public): \(addError.localizedDescription, privacy: .public)"
            )
            errorMessage = addError.localizedDescription
            Haptics.warning()
        } catch {
            Self.logger.error(
                "Unexpected error subscribing to typed URL \(trimmed, privacy: .public): \(error.localizedDescription, privacy: .public)"
            )
            errorMessage = error.localizedDescription
            Haptics.warning()
        }
    }

    private func isAlreadySubscribed(to suggestion: Suggestion) -> Bool {
        guard let url = URL(string: suggestion.feed),
              let podcast = store.podcast(feedURL: url) else { return false }
        return store.subscription(podcastID: podcast.id) != nil
    }
}

// MARK: - Suggestion model

extension OnboardingSubscribePage {
    /// A single curated podcast in the onboarding subscribe list.
    struct Suggestion: Identifiable, Hashable {
        let id = UUID()
        let title: String
        let author: String
        let feed: String
        let tint: Color
    }
}

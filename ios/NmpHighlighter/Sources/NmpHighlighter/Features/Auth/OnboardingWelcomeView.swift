import SwiftUI

struct OnboardingWelcomeView: View {
    @State private var page = 0

    private let pages: [PageContent] = [
        PageContent(
            icon: "highlighter",
            iconIsSystem: false,
            title: "Highlight what matters",
            body: "Save passages from articles, books, and podcasts — anywhere you read, listen, or watch."
        ),
        PageContent(
            icon: "person.2.fill",
            iconIsSystem: true,
            title: "See what your network loves",
            body: "Discover the passages your friends underline and the ideas shaping the conversations you care about."
        ),
        PageContent(
            icon: "books.vertical.fill",
            iconIsSystem: true,
            title: "Your reading life, portable",
            body: "Built on Nostr — open, encrypted, and yours. No platform can lock you out or disappear your highlights."
        ),
    ]

    var body: some View {
        ZStack {
            Color.highlighterPaper.ignoresSafeArea()

            VStack(spacing: 0) {
                TabView(selection: $page) {
                    ForEach(pages.indices, id: \.self) { idx in
                        pageView(pages[idx])
                            .tag(idx)
                    }
                }
                .tabViewStyle(.page(indexDisplayMode: .never))
                .frame(maxHeight: .infinity)

                VStack(spacing: 16) {
                    pageIndicator

                    if page == pages.count - 1 {
                        NavigationLink {
                            OnboardingCreateAccountView()
                        } label: {
                            Text("Get Started")
                                .font(.headline)
                                .frame(maxWidth: .infinity)
                                .padding(.vertical, 14)
                        }
                        .buttonStyle(.glassProminent)
                        .padding(.horizontal, 32)
                        .transition(.opacity.combined(with: .move(edge: .bottom)))
                    } else {
                        Button {
                            withAnimation { page += 1 }
                        } label: {
                            Text("Next")
                                .font(.headline)
                                .frame(maxWidth: .infinity)
                                .padding(.vertical, 14)
                        }
                        .buttonStyle(.glass)
                        .padding(.horizontal, 32)
                        .transition(.opacity)
                    }

                    NavigationLink {
                        LoginView()
                    } label: {
                        Text("Sign in with existing account")
                            .font(.footnote)
                            .foregroundStyle(Color.highlighterInkMuted)
                    }
                    .padding(.bottom, 4)
                }
                .padding(.bottom, 48)
                .animation(.easeInOut(duration: 0.2), value: page)
            }
        }
        .navigationBarHidden(true)
    }

    private func pageView(_ content: PageContent) -> some View {
        VStack(spacing: 0) {
            Spacer()

            if content.iconIsSystem {
                Image(systemName: content.icon)
                    .font(.system(size: 64, weight: .light))
                    .foregroundStyle(Color.highlighterAccent)
                    .padding(.bottom, 40)
            } else {
                Image(systemName: "bookmark.fill")
                    .font(.system(size: 56, weight: .light))
                    .foregroundStyle(Color.highlighterAccent)
                    .padding(.bottom, 40)
            }

            Text(content.title)
                .font(.system(.title, design: .default).weight(.semibold))
                .foregroundStyle(Color.highlighterInkStrong)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 32)
                .padding(.bottom, 16)

            Text(content.body)
                .font(.callout)
                .foregroundStyle(Color.highlighterInkMuted)
                .multilineTextAlignment(.center)
                .lineSpacing(4)
                .padding(.horizontal, 40)

            Spacer()
            Spacer()
        }
    }

    private var pageIndicator: some View {
        HStack(spacing: 6) {
            ForEach(pages.indices, id: \.self) { idx in
                Capsule()
                    .fill(idx == page ? Color.highlighterAccent : Color.highlighterInkMuted.opacity(0.3))
                    .frame(width: idx == page ? 20 : 6, height: 6)
                    .animation(.spring(response: 0.3), value: page)
            }
        }
    }

    private struct PageContent {
        let icon: String
        let iconIsSystem: Bool
        let title: String
        let body: String
    }
}

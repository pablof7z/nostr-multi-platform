import SwiftUI

// OWNER: Phase-2 Agent D — Wallet (polished "Coming in Chirp CX2" surface).
// No fake numbers. No wallet FFI at v1. Shows a preview-labeled locked balance
// card, NWC/zap/Cashu explainer, and the CX3 Olas-style auto-link teaser.

struct WalletView: View {
    @EnvironmentObject private var model: KernelModel

    var body: some View {
        ScrollView {
            VStack(spacing: ChirpSpace.xl) {
                balanceCard
                featureCards
                technologyCards
            }
            .padding(.horizontal, ChirpSpace.l)
            .padding(.top, ChirpSpace.m)
            .padding(.bottom, ChirpSpace.xxl)
        }
        .background(Color(.systemBackground))
        .navigationTitle("Wallet")
        .navigationBarTitleDisplayMode(.large)
    }

    // ── Locked balance card ───────────────────────────────────────────────

    private var balanceCard: some View {
        ZStack {
            // Gradient background
            RoundedRectangle(cornerRadius: ChirpSpace.radius, style: .continuous)
                .fill(
                    LinearGradient(
                        colors: [
                            ChirpColor.zap.opacity(0.22),
                            ChirpColor.accent.opacity(0.18),
                            Color(.systemBackground).opacity(0.0)
                        ],
                        startPoint: .topLeading,
                        endPoint: .bottomTrailing
                    )
                )
            RoundedRectangle(cornerRadius: ChirpSpace.radius, style: .continuous)
                .strokeBorder(
                    LinearGradient(
                        colors: [ChirpColor.zap.opacity(0.35), ChirpColor.accent.opacity(0.2)],
                        startPoint: .topLeading,
                        endPoint: .bottomTrailing
                    ),
                    lineWidth: 1
                )
            .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: ChirpSpace.radius, style: .continuous))

            VStack(spacing: ChirpSpace.xl) {
                // Preview badge
                HStack {
                    Spacer()
                    HStack(spacing: ChirpSpace.xs) {
                        Image(systemName: "lock.fill")
                            .font(.system(size: 10, weight: .bold))
                        Text("PREVIEW")
                            .font(.system(.caption2, design: .rounded).weight(.bold))
                            .tracking(1)
                    }
                    .foregroundStyle(ChirpColor.zap)
                    .padding(.horizontal, ChirpSpace.s)
                    .padding(.vertical, 4)
                    .background(ChirpColor.zap.opacity(0.15), in: Capsule())
                    .overlay(Capsule().strokeBorder(ChirpColor.zap.opacity(0.3)))
                }

                // Balance display — locked/blurred, never a real number
                VStack(spacing: ChirpSpace.s) {
                    Image(systemName: "bolt.circle.fill")
                        .font(.system(size: 44, weight: .light))
                        .foregroundStyle(
                            LinearGradient(
                                colors: [ChirpColor.zap, ChirpColor.zap.opacity(0.6)],
                                startPoint: .top, endPoint: .bottom
                            )
                        )
                        .symbolRenderingMode(.hierarchical)

                    // Locked balance placeholder — clearly marked as non-real
                    ZStack {
                        Text("— sats")
                            .font(.system(.largeTitle, design: .rounded).weight(.bold))
                            .foregroundStyle(ChirpColor.textPrimary)
                            .blur(radius: 8)

                        Image(systemName: "lock.fill")
                            .font(.system(size: 20, weight: .medium))
                            .foregroundStyle(ChirpColor.textTertiary)
                    }

                    Text("Balance unlocks in Chirp CX2")
                        .font(ChirpFont.caption)
                        .foregroundStyle(ChirpColor.textTertiary)
                }

                // CX2 version pill
                HStack(spacing: ChirpSpace.xs) {
                    Image(systemName: "sparkles")
                        .font(.system(size: 11, weight: .semibold))
                    Text("Coming in Chirp CX2")
                        .font(.system(.caption, design: .rounded).weight(.semibold))
                }
                .foregroundStyle(ChirpColor.accent)
                .padding(.horizontal, ChirpSpace.m)
                .padding(.vertical, ChirpSpace.xs)
                .background(ChirpColor.accentSoft, in: Capsule())
                .overlay(Capsule().strokeBorder(ChirpColor.accent.opacity(0.25)))
            }
            .padding(ChirpSpace.xl)
        }
    }

    // ── Feature cards ─────────────────────────────────────────────────────

    private var featureCards: some View {
        VStack(alignment: .leading, spacing: ChirpSpace.m) {
            ChirpSectionHeader(title: "What's Coming")

            VStack(spacing: ChirpSpace.m) {
                WalletFeatureRow(
                    icon: "bolt.fill",
                    iconColor: ChirpColor.zap,
                    cx: "CX2",
                    title: "Lightning Zaps",
                    description: "Send and receive sats instantly over Lightning with one tap. NWC-powered — bring your own wallet or connect to Alby, Zeus, or Mutiny."
                )
                WalletFeatureRow(
                    icon: "circle.hexagongrid.fill",
                    iconColor: Color(red: 0.85, green: 0.55, blue: 0.20),
                    cx: "CX2",
                    title: "Cashu Nutzaps",
                    description: "Ecash tokens for privacy-preserving tips via NIP-60. Receive zaps even while offline."
                )
                WalletFeatureRow(
                    icon: "link.circle.fill",
                    iconColor: ChirpColor.positive,
                    cx: "CX3",
                    title: "Identity–Wallet Auto-link",
                    description: "Olas-style seamless binding: your Nostr identity auto-links to your payment address. No manual LUD-16 copy-paste."
                )
                WalletFeatureRow(
                    icon: "chart.bar.xaxis",
                    iconColor: ChirpColor.accent,
                    cx: "CX2",
                    title: "Zap Analytics",
                    description: "See who zapped you, totals over time, and your most-appreciated notes — all in one glanceable dashboard."
                )
            }
        }
    }

    // ── Technology explainer ──────────────────────────────────────────────

    private var technologyCards: some View {
        VStack(alignment: .leading, spacing: ChirpSpace.m) {
            ChirpSectionHeader(title: "Powered By")

            HStack(spacing: ChirpSpace.m) {
                TechPill(label: "NWC", sublabel: "Nostr Wallet Connect", color: ChirpColor.zap)
                TechPill(label: "NIP-57", sublabel: "Zap protocol", color: ChirpColor.accent)
                TechPill(label: "Cashu", sublabel: "Ecash tokens", color: Color(red: 0.85, green: 0.55, blue: 0.20))
            }
        }
    }
}

// ── Wallet feature row ─────────────────────────────────────────────────────

private struct WalletFeatureRow: View {
    let icon: String
    let iconColor: Color
    let cx: String
    let title: String
    let description: String

    var body: some View {
        GlassCard {
            HStack(alignment: .top, spacing: ChirpSpace.m) {
                ZStack {
                    RoundedRectangle(cornerRadius: 10, style: .continuous)
                        .fill(iconColor.opacity(0.14))
                        .frame(width: 40, height: 40)
                    Image(systemName: icon)
                        .font(.system(size: 18, weight: .semibold))
                        .foregroundStyle(iconColor)
                        .symbolRenderingMode(.hierarchical)
                }

                VStack(alignment: .leading, spacing: ChirpSpace.xs) {
                    HStack(spacing: ChirpSpace.s) {
                        Text(title)
                            .font(ChirpFont.callout.weight(.semibold))
                            .foregroundStyle(ChirpColor.textPrimary)
                        Text(cx)
                            .font(.system(.caption2, design: .rounded).weight(.bold))
                            .foregroundStyle(.white)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 3)
                            .background(ChirpColor.accent, in: Capsule())
                    }
                    Text(description)
                        .font(ChirpFont.callout)
                        .foregroundStyle(ChirpColor.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }
}

// ── Technology pill ────────────────────────────────────────────────────────

private struct TechPill: View {
    let label: String
    let sublabel: String
    let color: Color

    var body: some View {
        GlassCard {
            VStack(spacing: ChirpSpace.xs) {
                Text(label)
                    .font(ChirpFont.headline)
                    .foregroundStyle(color)
                Text(sublabel)
                    .font(ChirpFont.caption)
                    .foregroundStyle(ChirpColor.textTertiary)
                    .multilineTextAlignment(.center)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, ChirpSpace.xs)
        }
    }
}

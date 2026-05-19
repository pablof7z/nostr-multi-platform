import SwiftUI

// MARK: - ux-15 token notes
//
// The brief (identity-05-synthesis §7) speaks ux-15 dialect. Mappings used
// throughout `Features/Identity/`:
//   - `display.large`     → AppTheme.Typography.largeTitle
//   - `headline`          → AppTheme.Typography.headline
//   - `subhead`           → AppTheme.Typography.subheadline
//   - `body`              → AppTheme.Typography.body
//   - `caption`           → AppTheme.Typography.caption
//   - `caption.small`     → AppTheme.Typography.caption2 (uppercase)
//   - `mono`              → AppTheme.Typography.monoCaption
//   - `glass.agent` tint  → AppTheme.Tint.agentSurface (indigo)
//   - `glass.clear`       → no tint (pure liquid glass)
//   - `accent.agent`      → AppTheme.Gradients.agentAccent
//   - `motion.standard / .considered / .snappy` → AppTheme.Animation.spring
//
// New York / SF Pro Rounded variants called out in the brief are mapped to
// AppTheme rounded tokens — we don't ship a custom serif font.

/// Mode badge — small T2 capsule that surfaces the user's signer flavour.
///
/// Per identity-05-synthesis §4.2: "From this page outward, every place that
/// shows the npub also shows the badge." The badge is shape + label, not
/// colour: the bunker mode adds a leading `link.icloud` glyph so the badge
/// is legible without colour cues.
struct ModeBadge: View {

    /// How the badge paints itself.
    enum Variant {
        /// Full T2 capsule used on the Identity root.
        case capsule
        /// Plain text fragment used in the Settings row second line — at
        /// list-row size, the tint pays off less than the saved horizontal
        /// space (per §4.1).
        case plain
    }

    let mode: UserIdentityStore.Mode
    var variant: Variant = .capsule

    var body: some View {
        switch variant {
        case .capsule: capsuleView
        case .plain:   plainView
        }
    }

    // MARK: - Variants

    private var capsuleView: some View {
        HStack(spacing: 4) {
            if isBunker {
                Image(systemName: "link.icloud")
                    .font(.system(size: 11, weight: .medium))
            }
            Text(label)
                .font(AppTheme.Typography.caption2.weight(.medium))
                .textCase(.uppercase)
                .tracking(0.4)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 4)
        .glassSurface(cornerRadius: AppTheme.Corner.pill, tint: tint, interactive: false)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(accessibilityLabel)
    }

    private var plainView: some View {
        HStack(spacing: 3) {
            if isBunker {
                Image(systemName: "link.icloud")
                    .font(.system(size: 10, weight: .medium))
            }
            Text(label)
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel(accessibilityLabel)
    }

    // MARK: - Derived

    private var isBunker: Bool { mode == .remoteSigner }

    private var label: String {
        switch mode {
        case .remoteSigner: "Bunker via Amber"
        case .localKey:     "Local Key"
        case .none:         "No Identity"
        }
    }

    private var tint: Color {
        // glass.agent is reserved for the bunker mode — the one defended
        // exception per ux-15 §7. Local key uses .clear (no tint).
        isBunker ? AppTheme.Tint.agentSurface.opacity(0.55) : Color.clear
    }

    private var accessibilityLabel: String {
        switch mode {
        case .remoteSigner: "Signed in with a remote signer"
        case .localKey:     "Local key on this device"
        case .none:         "No identity configured"
        }
    }
}

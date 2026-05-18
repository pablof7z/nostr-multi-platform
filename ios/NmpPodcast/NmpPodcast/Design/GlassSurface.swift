import SwiftUI

// MARK: - GlassSurface modifier

// Liquid Glass surface — iOS 26 native .glassEffect() with blur, reflection,
// and light interaction. The tinted overload keys color to semantic state.

struct GlassSurface: ViewModifier {
    var cornerRadius: CGFloat = AppTheme.Corner.lg
    var isInteractive: Bool = false

    func body(content: Content) -> some View {
        content
            .glassEffect(
                isInteractive ? .regular.interactive() : .regular,
                in: .rect(cornerRadius: cornerRadius)
            )
    }
}

extension View {
    func glassSurface(cornerRadius: CGFloat = AppTheme.Corner.lg, interactive: Bool = false) -> some View {
        modifier(GlassSurface(cornerRadius: cornerRadius, isInteractive: interactive))
    }

    func glassSurface(cornerRadius: CGFloat = AppTheme.Corner.lg, tint: Color, interactive: Bool = false) -> some View {
        self.glassEffect(
            interactive ? .regular.tint(tint).interactive() : .regular.tint(tint),
            in: .rect(cornerRadius: cornerRadius)
        )
    }
}


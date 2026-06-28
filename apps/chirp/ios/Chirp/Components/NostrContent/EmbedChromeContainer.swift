import SwiftUI

/// Visual chrome shared by every kind-dispatched embed renderer.
///
/// Mirrors the TUI's `EmbedChromeContainer`: a left accent stripe whose color
/// deepens with nesting depth, plus a small indent so embedded content reads
/// as a child of the surrounding paragraph. The renderer itself draws inside
/// `content` — chrome knows nothing about the embedded kind.
struct EmbedChromeContainer<Content: View>: View {
    var depth: UInt8
    var collapsed: Bool
    var content: () -> Content

    init(
        depth: UInt8 = 0,
        collapsed: Bool = false,
        @ViewBuilder content: @escaping () -> Content
    ) {
        self.depth = depth
        self.collapsed = collapsed
        self.content = content
    }

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            RoundedRectangle(cornerRadius: 1.5)
                .fill(accentColor)
                .frame(width: 3)
                .frame(maxHeight: .infinity)
            content()
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.leading, CGFloat(depth) * 8)
        .padding(.vertical, 4)
    }

    /// Depth-graded accent. Matches the TUI's `Rgb(140, 160 + 8·depth, 220)`
    /// blueish ramp; collapsed embeds dim out.
    private var accentColor: Color {
        if collapsed {
            return Color(red: 100/255, green: 100/255, blue: 110/255)
        }
        let green = min(200, 160 + Int(depth) * 8)
        return Color(
            red: 140/255,
            green: Double(green) / 255,
            blue: 220/255
        )
    }
}

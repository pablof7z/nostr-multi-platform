import AVKit
import SwiftUI
import UIKit

// MARK: - RoutePickerView

/// Thin SwiftUI wrapper around `AVRoutePickerView` so SwiftUI surfaces can
/// host the system's audio route picker. Tapping presents the system sheet
/// — the OS handles AirPlay / Bluetooth / USB-C selection without us
/// owning any AVAudioSession routing logic.
///
/// Tints are exposed so callers can render the glyph against the
/// surrounding chrome. By default the inner button's icon is a clear
/// AirPlay glyph the OS draws; pass `tintColor: .clear` to suppress it
/// when overlaying the picker on top of a custom chip (the picker still
/// captures taps even when its glyph is invisible).
struct RoutePickerView: UIViewRepresentable {
    var activeTintColor: UIColor = .tintColor
    var tintColor: UIColor = .label

    func makeUIView(context: Context) -> AVRoutePickerView {
        let view = AVRoutePickerView()
        view.prioritizesVideoDevices = false
        view.backgroundColor = .clear
        return view
    }

    func updateUIView(_ uiView: AVRoutePickerView, context: Context) {
        uiView.activeTintColor = activeTintColor
        uiView.tintColor = tintColor
    }
}

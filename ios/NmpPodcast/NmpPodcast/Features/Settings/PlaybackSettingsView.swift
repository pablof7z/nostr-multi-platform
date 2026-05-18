import SwiftUI

// MARK: - PlaybackSettingsView
//
// Settings → Playback. Exposes the small set of player preferences that almost
// every podcast app surfaces: default playback rate, transport skip intervals,
// and whether to auto-mark an episode played at end. All values write straight
// to `Settings`; `RootView` observes the same state and pushes updates into
// the live `AudioEngine` so changes take effect immediately.

struct PlaybackSettingsView: View {
    @Environment(AppStateStore.self) private var store

    /// Skip-interval choices match what the SF Symbol set ships (`gobackward.NN`
    /// / `goforward.NN`). Anything outside this list will fall back to the
    /// generic glyph in the player UI.
    private static let skipChoices: [Int] = [10, 15, 30, 45, 60, 75, 90]

    /// Allowed playback rates. Mirrors `PlaybackRate.allCases` so the slider
    /// snaps onto the same values the in-player speed sheet exposes.
    private static let rateChoices: [Double] = [0.8, 1.0, 1.2, 1.5, 2.0]

    var body: some View {
        Form {
            speedSection
            skipSection
            headphoneGestureSection
            autoMarkSection
            autoPlayNextSection
            autoSkipAdsSection
        }
        .navigationTitle("Playback")
        .navigationBarTitleDisplayMode(.inline)
    }

    // MARK: - Sections

    private var speedSection: some View {
        Section {
            Picker(selection: rateBinding) {
                ForEach(Self.rateChoices, id: \.self) { rate in
                    Text(formatRate(rate)).tag(rate)
                }
            } label: {
                Label("Default Speed", systemImage: "speedometer")
            }
            .pickerStyle(.menu)
        } header: {
            Text("Speed")
        } footer: {
            Text("Applied to new episodes. Use the player's speed control to override per session.")
        }
    }

    private var skipSection: some View {
        Section {
            Picker(selection: skipBackBinding) {
                ForEach(Self.skipChoices, id: \.self) { secs in
                    Text("\(secs) sec").tag(secs)
                }
            } label: {
                Label("Skip Back", systemImage: "gobackward")
            }
            .pickerStyle(.menu)

            Picker(selection: skipForwardBinding) {
                ForEach(Self.skipChoices, id: \.self) { secs in
                    Text("\(secs) sec").tag(secs)
                }
            } label: {
                Label("Skip Forward", systemImage: "goforward")
            }
            .pickerStyle(.menu)
        } header: {
            Text("Skip Intervals")
        } footer: {
            Text("Applied to in-app transport buttons and the lock-screen / Control Center skip controls.")
        }
    }

    private var autoMarkSection: some View {
        Section {
            Toggle(isOn: autoMarkBinding) {
                Label("Auto-mark played at end", systemImage: "checkmark.circle.fill")
            }
        } footer: {
            Text("When on, an episode is automatically marked played the first time playback reaches its end.")
        }
    }

    private var autoPlayNextSection: some View {
        Section {
            Toggle(isOn: autoPlayNextBinding) {
                Label("Auto-play next from queue", systemImage: "forward.end.fill")
            }
        } footer: {
            Text("When on, the next episode in your Up Next queue starts automatically when one finishes. The sleep timer's end-of-episode mode still stops playback as configured.")
        }
    }

    private var headphoneGestureSection: some View {
        Section {
            Picker(selection: doubleTapBinding) {
                ForEach(HeadphoneGestureAction.allCases, id: \.self) { action in
                    Text(gestureLabel(for: action)).tag(action)
                }
            } label: {
                Label("Double-Tap", systemImage: "airpods")
            }
            .pickerStyle(.menu)

            Picker(selection: tripleTapBinding) {
                ForEach(HeadphoneGestureAction.allCases, id: \.self) { action in
                    Text(gestureLabel(for: action)).tag(action)
                }
            } label: {
                Label("Triple-Tap", systemImage: "airpods")
            }
            .pickerStyle(.menu)
        } header: {
            Text("AirPods Gestures")
        } footer: {
            Text("Double-tap and triple-tap on AirPods (or stem squeeze on AirPods Pro / 3) trigger these actions. iOS also shows next/previous-track buttons on the lock screen, Control Center, and CarPlay — those buttons fire the same actions.")
        }
    }

    private var autoSkipAdsSection: some View {
        Section {
            Toggle(isOn: autoSkipAdsBinding) {
                Label("Auto-skip ads", systemImage: "speaker.slash.fill")
            }
        } footer: {
            Text("When on, the player seeks past ad reads detected from the transcript. Detection quality varies — leave off if you'd rather hear the host's sponsor breaks. Detected ads are still flagged on the chapter list either way.")
        }
    }

    // MARK: - Bindings

    private var rateBinding: Binding<Double> {
        Binding(
            get: { store.state.settings.defaultPlaybackRate },
            set: { v in
                var s = store.state.settings
                s.defaultPlaybackRate = v
                store.updateSettings(s)
                Haptics.selection()
            }
        )
    }

    private var skipBackBinding: Binding<Int> {
        Binding(
            get: { store.state.settings.skipBackwardSeconds },
            set: { v in
                var s = store.state.settings
                s.skipBackwardSeconds = v
                store.updateSettings(s)
                Haptics.selection()
            }
        )
    }

    private var skipForwardBinding: Binding<Int> {
        Binding(
            get: { store.state.settings.skipForwardSeconds },
            set: { v in
                var s = store.state.settings
                s.skipForwardSeconds = v
                store.updateSettings(s)
                Haptics.selection()
            }
        )
    }

    private var autoMarkBinding: Binding<Bool> {
        Binding(
            get: { store.state.settings.autoMarkPlayedAtEnd },
            set: { v in
                var s = store.state.settings
                s.autoMarkPlayedAtEnd = v
                store.updateSettings(s)
                Haptics.selection()
            }
        )
    }

    private var autoPlayNextBinding: Binding<Bool> {
        Binding(
            get: { store.state.settings.autoPlayNext },
            set: { v in
                var s = store.state.settings
                s.autoPlayNext = v
                store.updateSettings(s)
                Haptics.selection()
            }
        )
    }

    private var doubleTapBinding: Binding<HeadphoneGestureAction> {
        Binding(
            get: { store.state.settings.headphoneDoubleTapAction },
            set: { v in
                var s = store.state.settings
                s.headphoneDoubleTapAction = v
                store.updateSettings(s)
                Haptics.selection()
            }
        )
    }

    private var tripleTapBinding: Binding<HeadphoneGestureAction> {
        Binding(
            get: { store.state.settings.headphoneTripleTapAction },
            set: { v in
                var s = store.state.settings
                s.headphoneTripleTapAction = v
                store.updateSettings(s)
                Haptics.selection()
            }
        )
    }

    private var autoSkipAdsBinding: Binding<Bool> {
        Binding(
            get: { store.state.settings.autoSkipAds },
            set: { v in
                var s = store.state.settings
                s.autoSkipAds = v
                store.updateSettings(s)
                Haptics.selection()
            }
        )
    }

    // MARK: - Formatting

    /// Renders 1.0 as "1×" and other values as e.g. "1.5×".
    private func formatRate(_ rate: Double) -> String {
        if abs(rate - 1.0) < 0.001 { return "1×" }
        return String(format: "%.1f×", rate)
    }

    /// Gesture-action label that bakes in the live skip-second values so the
    /// menu reads e.g. "Skip Forward (30s)" — otherwise the picker rows lie
    /// when the user customizes the skip interval.
    private func gestureLabel(for action: HeadphoneGestureAction) -> String {
        switch action {
        case .skipForward:
            return "Skip Forward (\(store.state.settings.skipForwardSeconds)s)"
        case .skipBackward:
            return "Skip Back (\(store.state.settings.skipBackwardSeconds)s)"
        default:
            return action.displayName
        }
    }
}

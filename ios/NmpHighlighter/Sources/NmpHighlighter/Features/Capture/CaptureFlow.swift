import SwiftUI

/// Reusable capture entry point. Attach `.captureFlow(isPresented:preselectedGroupId:)`
/// to any screen that wants a "+" capture affordance — setting `isPresented`
/// to true opens the camera, and the modifier takes it from there (OCR →
/// review → publish). `preselectedGroupId` seeds `CaptureStore.selectedGroupId`
/// so a capture kicked off from inside a room defaults to publishing there.
extension View {
    func captureFlow(
        isPresented: Binding<Bool>,
        preselectedGroupId: String? = nil
    ) -> some View {
        modifier(CaptureFlowModifier(
            isPresented: isPresented,
            preselectedGroupId: preselectedGroupId
        ))
    }
}

private struct CaptureFlowModifier: ViewModifier {
    @Binding var isPresented: Bool
    let preselectedGroupId: String?

    @Environment(HighlighterStore.self) private var appStore
    @State private var store: CaptureStore?

    func body(content: Content) -> some View {
        content
            .task {
                if store == nil {
                    store = CaptureStore(safeCore: appStore.safeCore)
                }
            }
            .fullScreenCover(isPresented: cameraBinding) {
                CameraView { result in
                    isPresented = false
                    if case .captured(let image) = result, let store {
                        store.selectedGroupId = preselectedGroupId
                        store.handleCapturedImage(image)
                    }
                }
                .ignoresSafeArea()
            }
            .fullScreenCover(isPresented: reviewBinding) {
                if let store {
                    CapturePageView(
                        store: store,
                        onDismiss: { store.reset(keepingPickerSelection: false) }
                    )
                    .environment(appStore)
                }
            }
            .alert("Couldn't publish", isPresented: errorBinding, actions: {
                Button("OK") { store?.reset(keepingPickerSelection: true) }
            }, message: {
                if let store, case .error(let msg) = store.phase {
                    Text(msg)
                }
            })
            .onChange(of: store?.phase) { _, newValue in
                if case .done = newValue {
                    store?.reset(keepingPickerSelection: false)
                }
            }
    }

    private var cameraBinding: Binding<Bool> {
        Binding(
            get: { isPresented && store != nil },
            set: { presented in
                if !presented { isPresented = false }
            }
        )
    }

    private var reviewBinding: Binding<Bool> {
        Binding(
            get: {
                guard let store else { return false }
                switch store.phase {
                case .processing, .reviewing, .publishing: return true
                default: return false
                }
            },
            set: { presented in
                if !presented, let store, case .reviewing = store.phase {
                    store.reset(keepingPickerSelection: false)
                }
            }
        )
    }

    private var errorBinding: Binding<Bool> {
        Binding(
            get: {
                if let store, case .error = store.phase { return true }
                return false
            },
            set: { _ in }
        )
    }
}

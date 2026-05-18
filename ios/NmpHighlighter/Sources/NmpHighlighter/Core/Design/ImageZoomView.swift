import Kingfisher
import SwiftUI

struct ImageZoomView: View {
    let url: URL?
    let onDismiss: () -> Void

    @State private var scale: CGFloat = 1
    @State private var lastScale: CGFloat = 1
    @State private var dragOffset: CGSize = .zero

    private var dismissThreshold: CGFloat { 120 }

    var body: some View {
        ZStack {
            Color.black
                .opacity(1 - min(1, abs(dragOffset.height) / 300.0))
                .ignoresSafeArea()
            if let url {
                KFImage(url)
                    .placeholder { ProgressView().tint(.white) }
                    .resizable()
                    .scaledToFit()
                    .scaleEffect(scale)
                    .offset(dragOffset)
                    .gesture(
                        SimultaneousGesture(
                            MagnificationGesture()
                                .onChanged { value in
                                    scale = max(1, min(5, lastScale * value))
                                }
                                .onEnded { _ in lastScale = scale },
                            DragGesture()
                                .onChanged { value in
                                    guard scale == 1 else { return }
                                    dragOffset = value.translation
                                }
                                .onEnded { value in
                                    guard scale == 1 else {
                                        dragOffset = .zero
                                        return
                                    }
                                    if abs(value.translation.height) > dismissThreshold {
                                        onDismiss()
                                    } else {
                                        withAnimation(.spring(response: 0.3, dampingFraction: 0.8)) {
                                            dragOffset = .zero
                                        }
                                    }
                                }
                        )
                    )
            }
            VStack {
                HStack {
                    Spacer()
                    Button(action: onDismiss) {
                        Image(systemName: "xmark")
                            .font(.body.weight(.semibold))
                            .foregroundStyle(.white)
                            .padding(12)
                            .background(.ultraThinMaterial, in: Circle())
                    }
                    .padding()
                }
                Spacer()
            }
        }
    }
}

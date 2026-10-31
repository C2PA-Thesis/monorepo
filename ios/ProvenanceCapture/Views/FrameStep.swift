import SwiftUI

struct FrameStep: View {
    @Bindable var flow: CaptureFlow

    var body: some View {
        StepCard(
            number: 3, title: "Choose what to publish", summary: "The \(flow.crop.text) of the 1024×512 frame",
            state: flow.composeState
        ) {
            VStack(alignment: .leading, spacing: 12) {
                CropStage(image: flow.original?.image, crop: $flow.crop)
                if flow.shot?.photo.window.slidable == true {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Slide to frame the photo at 2:1").font(.subheadline).foregroundStyle(Theme.ink)
                        Slider(value: $flow.framePosition, in: 0...1)
                    }
                }
                HStack(spacing: 14) {
                    legend(Theme.publicInk, "Inside the box is published.")
                    legend(Theme.privateInk, "The rest stays private.")
                }
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        ForEach(CropPreset.all) { preset in
                            Chip(title: preset.name, selected: preset.rect == flow.crop) { flow.crop = preset.rect }
                        }
                    }
                }
                Hint("Drag inside the box to move it, or anywhere else to draw a new one. Now \(flow.crop.text).")
            }
        }
    }

    private func legend(_ color: Color, _ text: String) -> some View {
        HStack(spacing: 6) {
            RoundedRectangle(cornerRadius: 2).fill(color.opacity(0.35)).stroke(color).frame(width: 12, height: 12)
            Text(text).font(.caption).foregroundStyle(Theme.muted)
        }
    }
}

/// The 2:1 frame with the published rectangle over it, in original pixel coordinates.
struct CropStage: View {
    let image: CGImage?
    @Binding var crop: CropRect
    @State private var drag: CropDrag?

    var body: some View {
        GeometryReader { geometry in
            let width = geometry.size.width
            let scale = width / Double(Original.width)
            ZStack(alignment: .topLeading) {
                if let image {
                    Image(decorative: image, scale: 1).resizable().interpolation(.medium)
                } else {
                    Rectangle().fill(Theme.surface2).overlay(ProgressView())
                }
                // Dims what stays private.
                Rectangle().fill(.black.opacity(0.45))
                    .mask {
                        Rectangle().overlay(alignment: .topLeading) {
                            Rectangle().frame(width: Double(crop.width) * scale, height: Double(crop.height) * scale)
                                .offset(x: Double(crop.x) * scale, y: Double(crop.y) * scale)
                                .blendMode(.destinationOut)
                        }
                        .compositingGroup()
                    }
                Rectangle()
                    .stroke(Theme.publicInk, lineWidth: 2)
                    .frame(width: Double(crop.width) * scale, height: Double(crop.height) * scale)
                    .overlay(alignment: .topLeading) { Tag(text: "published", isPublic: true).padding(4) }
                    .offset(x: Double(crop.x) * scale, y: Double(crop.y) * scale)
            }
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { value in
                        let point = { (location: CGPoint) in
                            CropDrag.originalPoint(x: location.x, y: location.y, viewWidth: width)
                        }
                        let current = drag ?? CropDrag(start: point(value.startLocation), current: crop)
                        drag = current
                        // A tap alone changes nothing, as on the web page.
                        guard value.location != value.startLocation else { return }
                        crop = current.rect(to: point(value.location))
                    }
                    .onEnded { _ in drag = nil }
            )
        }
        .aspectRatio(2, contentMode: .fit)
        .clipShape(RoundedRectangle(cornerRadius: 8))
        .accessibilityElement()
        .accessibilityLabel("Published rectangle \(crop.text)")
    }
}

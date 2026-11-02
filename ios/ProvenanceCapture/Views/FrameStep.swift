import SwiftUI

struct FrameStep: View {
    @Bindable var flow: CaptureFlow

    var body: some View {
        StepCard(number: 3, title: "Crop", value: StepValue(flow.crop.text), state: flow.composeState, raised: false) {
            VStack(alignment: .leading, spacing: 12) {
                CropStage(image: flow.original?.image, crop: $flow.crop)
                if flow.shot?.photo.window.slidable == true {
                    BareSlider(
                        position: $flow.framePosition, range: 0...1, spoken: "Frame position",
                        value: "\(Int((flow.framePosition * 100).rounded()))%")
                }
                // The row bleeds to the card edge; its chips start and end on the content edge.
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        ForEach(CropPreset.all) { preset in
                            Chip(title: preset.name, selected: preset.rect == flow.crop) { flow.crop = preset.rect }
                        }
                    }
                }
                .contentMargins(.horizontal, 16, for: .scrollContent)
                .padding(.horizontal, -16)
                .padding(.vertical, -6)
                Note("The dimmed part stays private.")
            }
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
                Rectangle().fill(Theme.overlay)
                    .mask {
                        Rectangle().overlay(alignment: .topLeading) {
                            Rectangle().frame(width: Double(crop.width) * scale, height: Double(crop.height) * scale)
                                .offset(x: Double(crop.x) * scale, y: Double(crop.y) * scale)
                                .blendMode(.destinationOut)
                        }
                        .compositingGroup()
                    }
                // A white border with a thin ink line inside it reads on light and dark photos.
                // Only the sides inside the photo are drawn, so a preset on the photo edge
                // does not look inset from it.
                let outline = CropOutline(rect: CGRect(
                    x: Double(crop.x) * scale, y: Double(crop.y) * scale,
                    width: Double(crop.width) * scale, height: Double(crop.height) * scale))
                ZStack {
                    outline.stroke(Color(rgb: 0x0A0A0A).opacity(0.6), lineWidth: 6)
                    outline.stroke(.white, lineWidth: 4)
                }
                .clipShape(outline)
                .allowsHitTesting(false)
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
        .clipShape(CropOutline.photo)
        .accessibilityElement()
        .accessibilityLabel("Published rectangle \(crop.text)")
    }
}

/// The published rectangle in view points, pushed past the photo on the sides
/// that lie on its edge, so a border drawn inside it shows on the inner sides only.
private struct CropOutline: Shape {
    static let photo = RoundedRectangle(cornerRadius: Theme.Radius.control)
    var rect: CGRect

    func path(in bounds: CGRect) -> Path {
        let past: CGFloat = 8
        let near: CGFloat = 0.5
        var outline = rect
        if outline.minX <= bounds.minX + near { outline.origin.x -= past; outline.size.width += past }
        if outline.minY <= bounds.minY + near { outline.origin.y -= past; outline.size.height += past }
        if outline.maxX >= bounds.maxX - near { outline.size.width += past }
        if outline.maxY >= bounds.maxY - near { outline.size.height += past }
        return Path(outline)
    }
}

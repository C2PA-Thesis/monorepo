import SwiftUI

/// The photo editor: the picture, the current tool's options under it, and
/// the tools in a bar at the bottom. Only crop is proved today, so the other
/// tools are shown as coming soon.
struct CropEditor: View {
    let image: CGImage?
    let size: OriginalSize
    @Binding var crop: CropRect
    @State private var draft: CropRect
    @State private var tool = Tool.crop
    @State private var aspect = Aspect.free
    @Environment(\.dismiss) private var dismiss

    init(image: CGImage?, size: OriginalSize, crop: Binding<CropRect>) {
        self.image = image
        self.size = size
        _crop = crop
        _draft = State(initialValue: crop.wrappedValue)
    }

    enum Tool: CaseIterable, Identifiable {
        case crop, rotate, blur, greyscale

        var id: Self { self }

        var title: String {
            switch self {
            case .crop: "Crop"
            case .rotate: "Rotate"
            case .blur: "Blur"
            case .greyscale: "Greyscale"
            }
        }

        var icon: String {
            switch self {
            case .crop: "crop"
            case .rotate: "rotate.right"
            case .blur: "aqi.medium"
            case .greyscale: "circle.lefthalf.filled"
            }
        }

        var ready: Bool { self == .crop }
    }

    enum Aspect: CaseIterable, Identifiable {
        case free, square, fourThree, sixteenNine

        var id: Self { self }

        var title: String {
            switch self {
            case .free: "Free"
            case .square: "Square"
            case .fourThree: "4:3"
            case .sixteenNine: "16:9"
            }
        }

        /// Width over height, nil when the box can take any shape.
        var ratio: Double? {
            switch self {
            case .free: nil
            case .square: 1
            case .fourThree: 4.0 / 3
            case .sixteenNine: 16.0 / 9
            }
        }
    }

    var body: some View {
        VStack(spacing: 0) {
            topBar
            Spacer(minLength: 16)
            CropCanvas(image: image, size: size, crop: $draft, ratio: aspect.ratio, editable: tool == .crop)
                .padding(.horizontal, 20)
            Spacer(minLength: 16)
            options
                .frame(height: 52)
                .padding(.bottom, 12)
            toolBar
        }
        .background(Color.black.ignoresSafeArea())
        .preferredColorScheme(.dark)
        .onAppear { draft = crop }
    }

    private var topBar: some View {
        ZStack {
            HStack {
                Button("Cancel") { dismiss() }
                Spacer()
                Button("Done") {
                    crop = draft
                    dismiss()
                }
                .font(Typeface.sans(16, .bold, relativeTo: .headline))
            }
            if draft != .whole(size) {
                Button("Reset") {
                    withAnimation(Theme.ease) {
                        draft = .whole(size)
                        aspect = .free
                    }
                }
                .font(Typeface.sans(14, .semibold, relativeTo: .subheadline))
                .foregroundStyle(.white.opacity(0.7))
                .transition(.opacity)
            }
        }
        .font(Typeface.sans(16, .medium, relativeTo: .body))
        .foregroundStyle(.white)
        .padding(.horizontal, 20)
        .frame(height: 56)
    }

    @ViewBuilder private var options: some View {
        if tool == .crop {
            HStack(spacing: 8) {
                ForEach(Aspect.allCases) { option in
                    Chip(title: option.title, selected: aspect == option, dark: true) {
                        withAnimation(Theme.ease) {
                            aspect = option
                            if let ratio = option.ratio { draft = .fit(ratio, around: draft, in: size) }
                        }
                    }
                }
            }
        } else {
            Text("Coming soon")
                .font(Typeface.sans(15, .medium, relativeTo: .subheadline))
                .foregroundStyle(.white.opacity(0.6))
                .padding(.horizontal, 16)
                .frame(minHeight: 36)
                .background(.white.opacity(0.1), in: Capsule())
        }
    }

    private var toolBar: some View {
        HStack(spacing: 0) {
            ForEach(Tool.allCases) { item in
                Button {
                    withAnimation(Theme.ease) { tool = item }
                } label: {
                    VStack(spacing: 6) {
                        Image(systemName: item.icon)
                            .font(.system(size: 20, weight: .medium))
                            .frame(height: 26)
                            .overlay(alignment: .topTrailing) {
                                if !item.ready {
                                    Text("Soon")
                                        .font(Typeface.sans(9, .bold, relativeTo: .caption2))
                                        .padding(.horizontal, 5)
                                        .padding(.vertical, 2)
                                        .background(.white.opacity(0.18), in: Capsule())
                                        .fixedSize()
                                        .offset(x: 24, y: -8)
                                }
                            }
                        Text(item.title).font(Typeface.sans(12, .medium, relativeTo: .caption))
                        Circle().fill(tool == item ? .white : .clear).frame(width: 4, height: 4)
                    }
                    .foregroundStyle(tool == item ? .white : .white.opacity(item.ready ? 0.6 : 0.4))
                    .frame(maxWidth: .infinity, minHeight: 64)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(item.title)
                .accessibilityHint(item.ready ? "" : "Coming soon")
                .accessibilityAddTraits(tool == item ? .isSelected : [])
            }
        }
        .padding(.horizontal, 12)
        .padding(.top, 10)
        .padding(.bottom, 4)
        .background(Color.white.opacity(0.06).ignoresSafeArea(edges: .bottom))
    }
}

/// The photo with the crop box: drag a corner or an edge to resize, drag
/// inside to move. With a fixed shape only the corners resize.
private struct CropCanvas: View {
    let image: CGImage?
    let size: OriginalSize
    @Binding var crop: CropRect
    let ratio: Double?
    let editable: Bool
    @State private var start: CropRect?

    var body: some View {
        GeometryReader { geometry in
            let scale = geometry.size.width / Double(size.width)
            let box = CGRect(
                x: Double(crop.x) * scale, y: Double(crop.y) * scale,
                width: Double(crop.width) * scale, height: Double(crop.height) * scale)
            ZStack(alignment: .topLeading) {
                CropPreview(image: image, size: size, crop: crop)
                Rectangle().strokeBorder(.white, lineWidth: 1.5)
                    .frame(width: box.width, height: box.height)
                    .offset(x: box.minX, y: box.minY)
                if editable {
                    ThirdsGrid()
                        .stroke(.white.opacity(0.5), lineWidth: 0.75)
                        .frame(width: box.width, height: box.height)
                        .offset(x: box.minX, y: box.minY)
                        .opacity(start == nil ? 0 : 1)
                        .animation(Theme.ease, value: start == nil)
                        .allowsHitTesting(false)
                    Color.clear
                        .frame(width: box.width, height: box.height)
                        .contentShape(Rectangle())
                        .offset(x: box.minX, y: box.minY)
                        .gesture(move(scale))
                    ForEach(CropHandle.allCases.filter { ratio == nil || $0.isCorner }, id: \.self) { handle in
                        HandleMark(handle: handle)
                            .position(point(handle, box))
                            .gesture(resize(handle, scale))
                    }
                }
            }
        }
        .aspectRatio(Double(size.width) / Double(size.height), contentMode: .fit)
        .accessibilityElement()
        .accessibilityLabel(crop.isWhole(in: size) ? "Crop: the whole photo" : "Crop: part of the photo")
    }

    private func point(_ handle: CropHandle, _ box: CGRect) -> CGPoint {
        switch handle {
        case .topLeft: CGPoint(x: box.minX, y: box.minY)
        case .topRight: CGPoint(x: box.maxX, y: box.minY)
        case .bottomLeft: CGPoint(x: box.minX, y: box.maxY)
        case .bottomRight: CGPoint(x: box.maxX, y: box.maxY)
        case .top: CGPoint(x: box.midX, y: box.minY)
        case .bottom: CGPoint(x: box.midX, y: box.maxY)
        case .left: CGPoint(x: box.minX, y: box.midY)
        case .right: CGPoint(x: box.maxX, y: box.midY)
        }
    }

    private func move(_ scale: Double) -> some Gesture {
        DragGesture(minimumDistance: 1)
            .onChanged { value in
                let from = start ?? crop
                start = from
                let dx = Int((value.translation.width / scale).rounded())
                let dy = Int((value.translation.height / scale).rounded())
                crop = from.moved(dx: dx, dy: dy, in: size)
            }
            .onEnded { _ in start = nil }
    }

    private func resize(_ handle: CropHandle, _ scale: Double) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { value in
                let from = start ?? crop
                start = from
                let dx = Int((value.translation.width / scale).rounded())
                let dy = Int((value.translation.height / scale).rounded())
                crop = ratio.map { from.resized(corner: handle, dx: dx, dy: dy, ratio: $0, in: size) }
                    ?? from.resized(handle, dx: dx, dy: dy, in: size)
            }
            .onEnded { _ in start = nil }
    }
}

/// Lines at the thirds, shown while the box is being dragged.
private struct ThirdsGrid: Shape {
    func path(in rect: CGRect) -> Path {
        var path = Path()
        for third in [1.0 / 3, 2.0 / 3] {
            path.move(to: CGPoint(x: rect.minX + rect.width * third, y: rect.minY))
            path.addLine(to: CGPoint(x: rect.minX + rect.width * third, y: rect.maxY))
            path.move(to: CGPoint(x: rect.minX, y: rect.minY + rect.height * third))
            path.addLine(to: CGPoint(x: rect.maxX, y: rect.minY + rect.height * third))
        }
        return path
    }
}

/// A white corner bracket or edge bar with a 44 pt touch target.
private struct HandleMark: View {
    let handle: CropHandle

    var body: some View {
        Canvas { context, size in
            let center = CGPoint(x: size.width / 2, y: size.height / 2)
            var path = Path()
            switch handle {
            case .top, .bottom:
                path.move(to: CGPoint(x: center.x - 12, y: center.y))
                path.addLine(to: CGPoint(x: center.x + 12, y: center.y))
            case .left, .right:
                path.move(to: CGPoint(x: center.x, y: center.y - 12))
                path.addLine(to: CGPoint(x: center.x, y: center.y + 12))
            default:
                let arm: CGFloat = 18
                let dx: CGFloat = handle == .topLeft || handle == .bottomLeft ? 1 : -1
                let dy: CGFloat = handle == .topLeft || handle == .topRight ? 1 : -1
                path.move(to: CGPoint(x: center.x, y: center.y + dy * arm))
                path.addLine(to: center)
                path.addLine(to: CGPoint(x: center.x + dx * arm, y: center.y))
            }
            context.stroke(path, with: .color(.white), style: StrokeStyle(lineWidth: 4, lineCap: .round, lineJoin: .round))
        }
        .frame(width: 44, height: 44)
        .contentShape(Rectangle())
        .accessibilityHidden(true)
    }
}

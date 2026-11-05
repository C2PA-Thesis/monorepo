import SwiftUI

/// The photo editor: the picture, the current tool's options under it, and
/// the tools in a bar at the bottom. The laptop proves the crop and the turn
/// after it, so blur and greyscale are shown as coming soon.
struct CropEditor: View {
    let image: CGImage?
    let size: OriginalSize
    @Binding var crop: CropRect
    @Binding var turn: Turn
    @State private var draft: CropRect
    @State private var draftTurn: Turn
    /// Degrees on screen, not wrapped at 360 so the picture always spins the way it was turned.
    @State private var angle: Double
    @State private var tool = Tool.crop
    @State private var aspect = Aspect.free
    @Environment(\.dismiss) private var dismiss
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    init(image: CGImage?, size: OriginalSize, crop: Binding<CropRect>, turn: Binding<Turn>) {
        self.image = image
        self.size = size
        _crop = crop
        _turn = turn
        _draft = State(initialValue: crop.wrappedValue)
        _draftTurn = State(initialValue: turn.wrappedValue)
        _angle = State(initialValue: Double(turn.wrappedValue.degrees))
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

        var ready: Bool { self == .crop || self == .rotate }
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
            Group {
                if tool == .rotate {
                    TurnCanvas(image: image, crop: draft, turn: draftTurn, angle: angle)
                } else {
                    CropCanvas(image: image, size: size, crop: $draft, ratio: aspect.ratio, editable: tool == .crop)
                }
            }
            .padding(.horizontal, 20)
            Spacer(minLength: 16)
            options
                .frame(height: 52)
                .padding(.bottom, 12)
            toolBar
        }
        .background(Color.black.ignoresSafeArea())
        .preferredColorScheme(.dark)
        .onAppear {
            draft = crop
            draftTurn = turn
            angle = Double(turn.degrees)
        }
    }

    private var topBar: some View {
        ZStack {
            HStack {
                Button("Cancel") { dismiss() }
                Spacer()
                Button("Done") {
                    crop = draft
                    turn = draftTurn
                    dismiss()
                }
                .font(Typeface.sans(16, .bold, relativeTo: .headline))
            }
            if draft != .whole(size) || draftTurn != .zero {
                Button("Reset") {
                    withAnimation(Theme.ease) {
                        draft = .whole(size)
                        aspect = .free
                        draftTurn = .zero
                        // The nearest whole turn, so the picture does not spin back through every turn.
                        angle = (angle / 360).rounded() * 360
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
        if tool == .rotate {
            HStack(spacing: 8) {
                TurnButton(icon: "rotate.left", label: "Turn left") { turnBy(-1) }
                Text("\(draftTurn.degrees)°")
                    .font(Typeface.sans(15, .medium, relativeTo: .subheadline))
                    .monospacedDigit()
                    .foregroundStyle(.white.opacity(0.6))
                    .frame(minWidth: 56)
                    .accessibilityLabel(draftTurn == .zero ? "Not turned" : "Turned \(draftTurn.degrees) degrees clockwise")
                TurnButton(icon: "rotate.right", label: "Turn right") { turnBy(1) }
            }
        } else if tool == .crop {
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

    private func turnBy(_ quarters: Int) {
        withAnimation(reduceMotion ? nil : Theme.ease) {
            draftTurn = Turn(quarters: draftTurn.quarters + quarters)
            angle += Double(quarters) * 90
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

/// The published part alone, turned. The rectangle stays where it is in the
/// original; the turn only changes how its pixels are laid out.
private struct TurnCanvas: View {
    let image: CGImage?
    let crop: CropRect
    let turn: Turn
    let angle: Double

    var body: some View {
        GeometryReader { geometry in
            let shown = crop.size(turned: turn)
            let scale = min(geometry.size.width / Double(shown.width), geometry.size.height / Double(shown.height))
            // Drawn unturned and rotated on screen, so a turn animates; at rest it matches PublishedImage with the turn.
            PublishedImage(image: image, crop: crop, maxHeight: .infinity)
                .frame(width: Double(crop.width) * scale, height: Double(crop.height) * scale)
                .rotationEffect(.degrees(angle))
                .frame(width: geometry.size.width, height: geometry.size.height)
        }
        .accessibilityElement()
        .accessibilityLabel(turn == .zero ? "The photo, not turned" : "The photo, turned \(turn.degrees) degrees clockwise")
    }
}

/// An icon button in the style of a dark Chip.
private struct TurnButton: View {
    let icon: String
    let label: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: icon)
                .font(.system(size: 16, weight: .semibold))
                .frame(width: 52, height: 36)
                .foregroundStyle(.white)
                .background(.white.opacity(0.12), in: Capsule())
                .overlay(Capsule().strokeBorder(.white.opacity(0.18)))
                .padding(.vertical, 4)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(label)
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

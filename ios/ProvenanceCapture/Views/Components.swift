import MapKit
import SwiftUI

/// The verified mark: a solid green seal with a white check.
struct VerifiedMark: View {
    var size: CGFloat = 72

    var body: some View {
        ZStack {
            Image(systemName: "seal.fill").resizable().scaledToFit().foregroundStyle(Theme.verify)
            Image(systemName: "checkmark").font(.system(size: size * 0.36, weight: .bold)).foregroundStyle(.white)
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
    }
}

/// The PoC's ring spinner: a faint ring with a colored arc.
struct Spinner: View {
    var color = Theme.signal
    @State private var turning = false

    var body: some View {
        ZStack {
            Circle().stroke(Theme.line2, lineWidth: 2)
            Circle().trim(from: 0, to: 0.28).stroke(color, style: StrokeStyle(lineWidth: 2, lineCap: .round))
                .rotationEffect(.degrees(turning ? 360 : 0))
        }
        .onAppear { withAnimation(.linear(duration: 0.7).repeatForever(autoreverses: false)) { turning = true } }
        .accessibilityHidden(true)
    }
}

/// A selectable pill.
struct Chip: View {
    let title: String
    let selected: Bool
    var dark = false
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(title)
                .font(Typeface.sans(14, .semibold, relativeTo: .subheadline))
                .padding(.horizontal, 14)
                .frame(minHeight: 36)
                .foregroundStyle(foreground)
                .background(background, in: Capsule())
                .overlay(Capsule().strokeBorder(selected ? .clear : border))
                .padding(.vertical, 4)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }

    private var foreground: Color { dark ? (selected ? .black : .white) : (selected ? Theme.onInk : Theme.ink) }
    private var background: Color { dark ? (selected ? .white : .white.opacity(0.12)) : (selected ? Theme.ink : Theme.paper) }
    private var border: Color { dark ? .white.opacity(0.18) : Theme.line2 }
}

/// A short message with a colored mark: what went wrong or needs attention.
struct Callout: View {
    enum Tone { case danger, warn, signal }
    let tone: Tone
    let title: String
    var message: String?

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: icon).font(.system(size: 16, weight: .semibold)).foregroundStyle(color).padding(.top, 1)
            VStack(alignment: .leading, spacing: 3) {
                Text(title).font(Typeface.sans(15, .semibold, relativeTo: .headline)).foregroundStyle(Theme.ink)
                if let message {
                    Text(message).font(Typeface.sans(14, relativeTo: .subheadline)).foregroundStyle(Theme.ink2)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            Spacer(minLength: 0)
        }
        .padding(14)
        .background(soft, in: RoundedRectangle(cornerRadius: Theme.Radius.sm))
        .accessibilityElement(children: .combine)
    }

    private var icon: String {
        switch tone {
        case .danger: "exclamationmark.circle.fill"
        case .warn: "exclamationmark.triangle.fill"
        case .signal: "info.circle.fill"
        }
    }

    private var color: Color {
        switch tone {
        case .danger: Theme.danger
        case .warn: Theme.warn
        case .signal: Theme.signal
        }
    }

    private var soft: Color {
        switch tone {
        case .danger: Theme.dangerSoft
        case .warn: Theme.warnSoft
        case .signal: Theme.signalSoft
        }
    }
}

/// Opens this app's page in Settings.
struct OpenSettingsButton: View {
    var style = PillButtonStyle(kind: .ghost)

    var body: some View {
        if let url = URL(string: UIApplication.openSettingsURLString) {
            Link(destination: url) { Text("Open Settings") }.buttonStyle(style)
        }
    }
}

/// The area on a muted map: the hexagon in teal, and on this phone only, a
/// dot where the photo was taken.
struct AreaMap: View {
    let outline: Outline?
    var you: CLLocationCoordinate2D?
    /// Metres around `you` while the outline is not known yet.
    var span: Double = 4000
    /// Nil lets the map take the room it is given.
    var height: CGFloat? = 240
    @State private var camera = MapCameraPosition.automatic

    var body: some View {
        Map(position: $camera, interactionModes: []) {
            if let outline {
                MapPolygon(coordinates: Self.coordinates(outline))
                    .foregroundStyle(Theme.signalBright.opacity(0.16))
                    .stroke(Theme.signal, style: StrokeStyle(lineWidth: 2.5, lineJoin: .round))
            }
            if let you {
                Annotation("Where you took it", coordinate: you) {
                    Circle().fill(Theme.ink).frame(width: 10, height: 10)
                        .overlay(Circle().strokeBorder(Theme.paper, lineWidth: 2.5).padding(-2.5))
                }
                .annotationTitles(.hidden)
            }
        }
        .mapStyle(.standard(emphasis: .muted, pointsOfInterest: .excludingAll))
        .mapControls {}
        .frame(height: height)
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.md))
        .overlay(RoundedRectangle(cornerRadius: Theme.Radius.md).strokeBorder(Theme.line))
        .onChange(of: outline?.cell, initial: true) { _, _ in fit() }
        .accessibilityElement()
        .accessibilityLabel(outline.map { "Map of the shared area, about \(Verdict.area($0.areaKm2))" } ?? "Map")
    }

    static func coordinates(_ outline: Outline) -> [CLLocationCoordinate2D] {
        outline.boundary.compactMap { pair in
            pair.count == 2 ? CLLocationCoordinate2D(latitude: pair[0], longitude: pair[1]) : nil
        }
    }

    private func fit() {
        guard let outline else {
            if let you {
                camera = .region(MKCoordinateRegion(center: you, latitudinalMeters: span, longitudinalMeters: span))
            }
            return
        }
        let points = Self.coordinates(outline).map(MKMapPoint.init)
        guard let first = points.first else { return }
        var rect = MKMapRect(origin: first, size: MKMapSize(width: 0, height: 0))
        for point in points.dropFirst() { rect = rect.union(MKMapRect(origin: point, size: MKMapSize(width: 0, height: 0))) }
        let pad = max(rect.width, rect.height) * 0.22
        withAnimation(Theme.ease) { camera = .rect(rect.insetBy(dx: -pad, dy: -pad)) }
    }
}

/// Only the part of the original that gets published.
struct PublishedImage: View {
    let image: CGImage?
    let crop: CropRect
    var maxHeight: CGFloat = 420

    var body: some View {
        FitBox(ratio: CGFloat(crop.width) / CGFloat(crop.height), maxHeight: maxHeight) {
            // The image is the original, so the crop is already in its pixels.
            if let image, let part = image.cropping(to: CGRect(x: crop.x, y: crop.y, width: crop.width, height: crop.height)) {
                Image(decorative: part, scale: 1).resizable().interpolation(.medium)
            } else {
                Theme.surface2
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.md))
    }
}

/// Sizes its child to a ratio: full width, unless that is taller than `maxHeight`.
private struct FitBox: Layout {
    let ratio: CGFloat
    let maxHeight: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? maxHeight * ratio
        let height = min(width / ratio, maxHeight)
        return CGSize(width: height * ratio, height: height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        for subview in subviews {
            subview.place(at: bounds.origin, proposal: ProposedViewSize(bounds.size))
        }
    }
}

/// The original with the private part shaded, for the editor.
struct CropPreview: View {
    let image: CGImage?
    let size: OriginalSize
    let crop: CropRect
    var shade = true

    var body: some View {
        GeometryReader { geometry in
            let scale = geometry.size.width / Double(size.width)
            ZStack(alignment: .topLeading) {
                if let image {
                    Image(decorative: image, scale: 1).resizable().interpolation(.medium)
                } else {
                    Rectangle().fill(Theme.surface2)
                }
                if shade, !crop.isWhole(in: size) {
                    Theme.shade.mask {
                        Rectangle().overlay(alignment: .topLeading) {
                            Rectangle()
                                .frame(width: Double(crop.width) * scale, height: Double(crop.height) * scale)
                                .offset(x: Double(crop.x) * scale, y: Double(crop.y) * scale)
                                .blendMode(.destinationOut)
                        }
                        .compositingGroup()
                    }
                    Rectangle().strokeBorder(.white, lineWidth: 2)
                        .frame(width: Double(crop.width) * scale, height: Double(crop.height) * scale)
                        .offset(x: Double(crop.x) * scale, y: Double(crop.y) * scale)
                }
            }
        }
        .aspectRatio(Double(size.width) / Double(size.height), contentMode: .fit)
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.md))
    }
}

/// A screen's heading: a large title and one line under it.
struct Heading: View {
    let title: String
    var subtitle: String?
    var center = false

    var body: some View {
        VStack(alignment: center ? .center : .leading, spacing: 8) {
            Text(title).font(Typeface.title).tight(26).foregroundStyle(Theme.ink)
                .accessibilityAddTraits(.isHeader)
            if let subtitle {
                Text(subtitle).font(Typeface.body).foregroundStyle(Theme.soft).lineSpacing(2)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .multilineTextAlignment(center ? .center : .leading)
        .frame(maxWidth: .infinity, alignment: center ? .center : .leading)
    }
}

/// Names for the area sizes.
enum AreaWords {
    /// Short names for the size picker, coarse to fine.
    static let names: [Int: String] = [
        4: "Region", 5: "City", 6: "District", 7: "Neighbourhood", 8: "A few blocks", 9: "One block",
    ]
}

/// The app's mark: a camera aperture whose six blades leave a hexagon open,
/// the shape of the area a photo shares.
struct ApertureMark: View, Animatable {
    /// The hexagon's size against the lens, 0 closed to 1 wide open.
    var opening: Double = 0.5
    var rotation: Double = 0

    nonisolated var animatableData: AnimatablePair<Double, Double> {
        get { AnimatablePair(opening, rotation) }
        set {
            opening = newValue.first
            rotation = newValue.second
        }
    }

    var body: some View {
        Canvas { context, size in
            let blades = Aperture.blades(in: CGRect(origin: .zero, size: size), opening: opening, rotation: rotation)
            let radius = min(size.width, size.height) / 2
            for (index, blade) in blades.enumerated() {
                let colors = [Theme.iris[index], Theme.iris[index + 1]]
                context.fill(
                    blade.path,
                    with: .linearGradient(Gradient(colors: colors), startPoint: blade.from, endPoint: blade.to))
            }
            // Thin gaps between the blades, cut through to whatever is behind.
            context.blendMode = .destinationOut
            for blade in blades {
                context.stroke(blade.path, with: .color(.black), style: StrokeStyle(lineWidth: radius * 0.05, lineJoin: .round))
            }
        }
        .aspectRatio(1, contentMode: .fit)
        .accessibilityHidden(true)
    }
}

/// The mark turning slowly while something runs.
struct ApertureLoader: View {
    var size: CGFloat = 96
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        TimelineView(.animation(paused: reduceMotion)) { context in
            let time = context.date.timeIntervalSinceReferenceDate
            ApertureMark(opening: 0.4 + 0.1 * sin(time * 1.6), rotation: reduceMotion ? 0 : time * 0.5)
        }
        .frame(width: size, height: size)
        .accessibilityElement()
        .accessibilityLabel("Working")
    }
}

enum Aperture {
    struct Blade {
        let path: Path
        let from: CGPoint
        let to: CGPoint
    }

    /// Six blades around a hexagon. Each blade runs from one hexagon edge out
    /// to the lens along the extensions of that edge and the one before it.
    static func blades(in rect: CGRect, opening: Double, rotation: Double) -> [Blade] {
        let center = CGPoint(x: rect.midX, y: rect.midY)
        let lens = min(rect.width, rect.height) / 2
        let inner = lens * min(max(opening, 0.05), 0.95)
        let corners = (0..<6).map { index in
            let angle = rotation - .pi / 2 + Double(index) * .pi / 3
            return CGPoint(x: center.x + inner * cos(angle), y: center.y + inner * sin(angle))
        }
        // Where the edge from corner k to k+1, carried on past k+1, meets the lens.
        let exits = (0..<6).map { index in
            let a = corners[index]
            let b = corners[(index + 1) % 6]
            let length = hypot(b.x - a.x, b.y - a.y)
            let d = CGPoint(x: (b.x - a.x) / length, y: (b.y - a.y) / length)
            let p = CGPoint(x: b.x - center.x, y: b.y - center.y)
            let dot = p.x * d.x + p.y * d.y
            let t = -dot + (dot * dot - (p.x * p.x + p.y * p.y - lens * lens)).squareRoot()
            return CGPoint(x: b.x + t * d.x, y: b.y + t * d.y)
        }
        return (0..<6).map { index in
            let previous = exits[(index + 5) % 6]
            let exit = exits[index]
            var path = Path()
            path.move(to: corners[index])
            path.addLine(to: corners[(index + 1) % 6])
            path.addLine(to: exit)
            var start = atan2(exit.y - center.y, exit.x - center.x)
            let end = atan2(previous.y - center.y, previous.x - center.x)
            if start < end { start += 2 * .pi }
            for step in 1...24 {
                let angle = start + (end - start) * Double(step) / 24
                path.addLine(to: CGPoint(x: center.x + lens * cos(angle), y: center.y + lens * sin(angle)))
            }
            path.closeSubpath()
            return Blade(path: path, from: previous, to: exit)
        }
    }
}

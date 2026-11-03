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
            if let image, let part = image.cropping(to: rect(in: image)) {
                Image(decorative: part, scale: 1).resizable().interpolation(.medium)
            } else {
                Theme.surface2
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.md))
    }

    private func rect(in image: CGImage) -> CGRect {
        let scale = Double(image.width) / Double(Original.width)
        return CGRect(
            x: Double(crop.x) * scale, y: Double(crop.y) * scale,
            width: Double(crop.width) * scale, height: Double(crop.height) * scale)
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

/// The 2:1 original with the private part shaded, for the editor.
struct CropPreview: View {
    let image: CGImage?
    let crop: CropRect
    var shade = true

    var body: some View {
        GeometryReader { geometry in
            let scale = geometry.size.width / Double(Original.width)
            ZStack(alignment: .topLeading) {
                if let image {
                    Image(decorative: image, scale: 1).resizable().interpolation(.medium)
                } else {
                    Rectangle().fill(Theme.surface2)
                }
                if shade, !crop.isWhole {
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
        .aspectRatio(2, contentMode: .fit)
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

/// The app's mark: a viewfinder's four corners around the hexagon a photo
/// shares. Same drawing as web/src/mark.ts, in a 100 by 100 box.
struct ViewfinderMark: View, Animatable {
    /// The corners' size, 1 at rest and smaller as they close in.
    var corners: Double = 1
    /// The hexagon's opacity.
    var glow: Double = 1

    nonisolated var animatableData: AnimatablePair<Double, Double> {
        get { AnimatablePair(corners, glow) }
        set {
            corners = newValue.first
            glow = newValue.second
        }
    }

    var body: some View {
        Canvas { context, size in
            let side = min(size.width, size.height)
            context.translateBy(x: (size.width - side) / 2, y: (size.height - side) / 2)
            context.scaleBy(x: side / 100, y: side / 100)
            var brackets = context
            brackets.translateBy(x: 50, y: 50)
            brackets.scaleBy(x: corners, y: corners)
            brackets.translateBy(x: -50, y: -50)
            brackets.stroke(Viewfinder.brackets, with: .color(Theme.ink), style: StrokeStyle(lineWidth: 8.5, lineCap: .round, lineJoin: .round))
            // One layer, so the fill and the stroke fade together without a darker seam.
            context.opacity = glow
            context.drawLayer { layer in
                layer.fill(Viewfinder.hexagon, with: .color(Theme.signal))
                layer.stroke(Viewfinder.hexagon, with: .color(Theme.signal), style: StrokeStyle(lineWidth: 5, lineJoin: .round))
            }
        }
        .aspectRatio(1, contentMode: .fit)
        .accessibilityHidden(true)
    }
}

/// The mark while something runs: the corners breathe and the hexagon blinks.
struct ViewfinderLoader: View {
    var size: CGFloat = 96
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        TimelineView(.animation(paused: reduceMotion)) { context in
            let time = reduceMotion ? 0 : context.date.timeIntervalSinceReferenceDate
            ViewfinderMark(
                corners: 0.92 + 0.08 * cos(2 * .pi * time / 2.8),
                glow: 0.65 + 0.35 * cos(2 * .pi * time / 2.4))
        }
        .frame(width: size, height: size)
        .accessibilityElement()
        .accessibilityLabel("Working")
    }
}

enum Viewfinder {
    /// Four corners, each 18 long on both sides with a radius of 6.
    static let brackets: Path = {
        var path = Path()
        for (x, y, dx, dy) in [(14.0, 14.0, 1.0, 1.0), (86, 14, -1, 1), (86, 86, -1, -1), (14, 86, 1, -1)] {
            path.move(to: CGPoint(x: x, y: y + 18 * dy))
            path.addArc(tangent1End: CGPoint(x: x, y: y), tangent2End: CGPoint(x: x + 18 * dx, y: y), radius: 6)
            path.addLine(to: CGPoint(x: x + 18 * dx, y: y))
        }
        return path
    }()

    static let hexagon: Path = {
        var path = Path()
        path.addLines([(50, 30), (67.3, 40), (67.3, 60), (50, 70), (32.7, 60), (32.7, 40)].map { CGPoint(x: $0.0, y: $0.1) })
        path.closeSubpath()
        return path
    }()
}

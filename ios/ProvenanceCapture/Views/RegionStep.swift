import MapKit
import SwiftUI

struct RegionStep: View {
    @Bindable var flow: CaptureFlow

    var body: some View {
        StepCard(number: 4, title: "Area", value: flow.regionSummary, state: flow.composeState, raised: false) {
            VStack(alignment: .leading, spacing: 12) {
                CellMap(
                    outline: nil, you: flow.shot.map { CLLocationCoordinate2D(latitude: $0.fix.latitude, longitude: $0.fix.longitude) },
                    span: flow.cellWidth * 3)
                LabeledSlider(
                    label: "Size", value: flow.resolutionText,
                    position: Binding(
                        get: { Double(flow.resolution) },
                        set: { flow.resolution = Int($0.rounded()) }),
                    range: Double(Verdict.resolutions.lowerBound)...Double(Verdict.resolutions.upperBound), step: 1,
                    spoken: "Area size")
                if flow.cellFailed { Note(flow.cellText, tone: Theme.bad) }
                if let warning = flow.accuracyWarning { Note(warning, tone: Theme.warn) }
            }
        }
    }
}

/// The H3 cell on a map, in grayscale as on the web. The capture screen also
/// shows the phone its own position. The map does not pan or zoom, so a swipe
/// on it scrolls the page and the cell or the dot never leaves the frame.
struct CellMap: View {
    let outline: Outline?
    var you: CLLocationCoordinate2D?
    /// Metres shown around `you` when there is no outline.
    var span: Double = 4000
    @State private var camera = MapCameraPosition.automatic

    var body: some View {
        Map(position: $camera, interactionModes: []) {
            if let outline {
                MapPolygon(coordinates: coordinates(outline))
                    .foregroundStyle(Theme.ink.opacity(0.06))
                    .stroke(Theme.ink, lineWidth: 1.5)
            }
            if let you {
                Annotation("Your position, never published", coordinate: you) {
                    // Ink with a surface ring: dark on light tiles, light on dark tiles.
                    Circle().fill(Theme.ink).frame(width: 12, height: 12)
                        .overlay(Circle().strokeBorder(Theme.surface, lineWidth: 2.5).padding(-2.5))
                }
                .annotationTitles(.hidden)
            }
        }
        .mapStyle(.standard(emphasis: .muted, pointsOfInterest: .excludingAll))
        .mapControls {}
        .saturation(0)
        .contrast(0.92)
        .frame(height: 180)
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.control))
        .overlay(RoundedRectangle(cornerRadius: Theme.Radius.control).strokeBorder(Theme.line))
        .onChange(of: outline?.cell, initial: true) { _, _ in fit() }
        .onChange(of: span) { _, _ in fit() }
    }

    private func coordinates(_ outline: Outline) -> [CLLocationCoordinate2D] {
        outline.boundary.compactMap { pair in
            pair.count == 2 ? CLLocationCoordinate2D(latitude: pair[0], longitude: pair[1]) : nil
        }
    }

    private func fit() {
        guard let outline else {
            if let you {
                withAnimation { camera = .region(MKCoordinateRegion(center: you, latitudinalMeters: span, longitudinalMeters: span)) }
            }
            return
        }
        let points = coordinates(outline).map(MKMapPoint.init)
        guard let first = points.first else { return }
        var rect = MKMapRect(origin: first, size: MKMapSize(width: 0, height: 0))
        for point in points.dropFirst() { rect = rect.union(MKMapRect(origin: point, size: MKMapSize(width: 0, height: 0))) }
        let pad = max(rect.width, rect.height) * 0.15
        withAnimation { camera = .rect(rect.insetBy(dx: -pad, dy: -pad)) }
    }
}

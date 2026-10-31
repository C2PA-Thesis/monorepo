import MapKit
import SwiftUI

struct RegionStep: View {
    @Bindable var flow: CaptureFlow

    var body: some View {
        StepCard(
            number: 4, title: "Choose how precisely to reveal the place", summary: flow.regionSummary,
            state: flow.composeState
        ) {
            VStack(alignment: .leading, spacing: 12) {
                CellMap(
                    outline: nil, you: flow.shot.map { CLLocationCoordinate2D(latitude: $0.fix.latitude, longitude: $0.fix.longitude) },
                    span: flow.cellWidth * 3)
                VStack(alignment: .leading, spacing: 4) {
                    HStack {
                        Text("Area size").font(.subheadline)
                        Spacer()
                        Text(flow.resolutionText).font(.subheadline.weight(.semibold))
                    }
                    Slider(
                        value: Binding(
                            get: { Double(flow.resolution) },
                            set: { flow.resolution = Int($0.rounded()) }),
                        in: Double(Verdict.resolutions.lowerBound)...Double(Verdict.resolutions.upperBound), step: 1)
                    HStack {
                        Text("Larger, more private")
                        Spacer()
                        Text("Smaller, more precise")
                    }
                    .font(.caption).foregroundStyle(Theme.muted)
                }
                if !flow.cellText.isEmpty { Hint(flow.cellText, tone: flow.cellFailed ? Theme.bad : Theme.muted) }
                if let warning = flow.accuracyWarning { Hint(warning, tone: Theme.privateInk) }
            }
        }
    }
}

/// The H3 cell on a map. The capture screen also shows the phone its own position.
struct CellMap: View {
    let outline: Outline?
    var you: CLLocationCoordinate2D?
    /// Metres shown around `you` when there is no outline.
    var span: Double = 4000
    @State private var camera = MapCameraPosition.automatic

    var body: some View {
        Map(position: $camera, interactionModes: [.pan, .zoom]) {
            if let outline {
                MapPolygon(coordinates: coordinates(outline))
                    .foregroundStyle(Theme.accent.opacity(0.18))
                    .stroke(Theme.accent, lineWidth: 2)
            }
            if let you {
                Annotation("You, exactly. Never published.", coordinate: you) {
                    Circle().fill(Theme.you).stroke(.white, lineWidth: 2).frame(width: 14, height: 14)
                }
                .annotationTitles(.hidden)
            }
        }
        .frame(height: 240)
        .clipShape(RoundedRectangle(cornerRadius: 10))
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

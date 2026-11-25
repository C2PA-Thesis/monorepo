import CoreLocation
import MapKit
import SwiftUI

/// Right after the shutter. The phone seals the photo in a second or two, so
/// this is a loader; it only says more when something goes wrong.
struct SealScreen: View {
    let flow: CaptureFlow

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                CloseButton { flow.retake() }
                Spacer()
            }
            .padding(.horizontal, 16)
            .frame(height: 56)
            Spacer()
            if let failure = flow.sealFailure {
                Problem(title: failure.title, message: failure.message)
            } else {
                LoadingState(title: "Signing your photo")
            }
            Spacer()
            if let failure = flow.sealFailure {
                VStack(spacing: 10) {
                    if failure.action == .settings {
                        OpenSettingsButton(style: PillButtonStyle(kind: .primary))
                        Button("Take it again") { flow.retake() }.buttonStyle(.quiet)
                    } else {
                        Button("Take it again") { flow.retake() }.buttonStyle(.primary)
                    }
                }
                .padding(.horizontal, 24)
                .padding(.bottom, 12)
            }
        }
        .background(Theme.paper.ignoresSafeArea())
    }
}

/// A failure, centred: a mark, what happened and what to do.
struct Problem: View {
    let title: String
    let message: String
    @State private var shown = false

    var body: some View {
        VStack(spacing: 20) {
            Image(systemName: "exclamationmark").font(.system(size: 24, weight: .bold)).foregroundStyle(Theme.danger)
                .symbolEffect(.bounce, value: shown)
                .frame(width: 64, height: 64).background(Theme.dangerSoft, in: Circle())
                .scaleEffect(shown ? 1 : 0.6)
                .opacity(shown ? 1 : 0)
                .accessibilityHidden(true)
            Heading(title: title, subtitle: message, center: true)
        }
        .padding(.horizontal, 32)
        .sensoryFeedback(.error, trigger: shown) { _, new in new }
        .onAppear {
            withAnimation(.spring(duration: 0.4, bounce: 0.35)) { shown = true }
            Sound.error.play()
        }
    }
}

struct CloseButton: View {
    var label = "Discard"
    var icon = "xmark"
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: icon).font(.system(size: 14, weight: .bold)).foregroundStyle(Theme.ink)
                .frame(width: 40, height: 40).background(Theme.surface, in: Circle())
        }
        .accessibilityLabel(label)
    }
}

/// Step one after the seal: the photo, and optionally the part of it to share.
struct PhotoScreen: View {
    @Bindable var flow: CaptureFlow
    @State private var cropping = false
    @State private var discarding = false

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                CloseButton { discarding = true }
                Spacer()
            }
            .padding(.horizontal, 16)
            .frame(height: 56)
            VStack(alignment: .leading, spacing: 20) {
                Heading(title: "Ready to publish", subtitle: editNote)
                    .animation(Theme.ease, value: editNote)
                PublishedImage(image: flow.preview, crop: flow.crop, turn: flow.turn)
                    .frame(maxWidth: .infinity)
                    .overlay(alignment: .bottomTrailing) {
                        Image(systemName: "crop")
                            .font(.system(size: 14, weight: .semibold))
                            .foregroundStyle(.white)
                            .frame(width: 34, height: 34)
                            .background(.black.opacity(0.55), in: Circle())
                            .padding(10)
                            .accessibilityHidden(true)
                    }
                    .onTapGesture { cropping = true }
                    .accessibilityAddTraits(.isButton)
                    .accessibilityHint("Edit the photo")
            }
            .padding(.horizontal, 24)
            .padding(.top, 8)
            Spacer(minLength: 24)
            HStack(spacing: 10) {
                Button {
                    cropping = true
                } label: {
                    Label("Edit", systemImage: "slider.horizontal.3")
                }
                .buttonStyle(.ghost)
                .frame(maxWidth: 140)
                Button("Publish") { withAnimation(Theme.ease) { flow.photoChosen = true } }
                    .buttonStyle(.primary)
            }
            .padding(.horizontal, 24)
            .padding(.bottom, 12)
        }
        .background(Theme.paper.ignoresSafeArea())
        .fullScreenCover(isPresented: $cropping) { CropEditor(image: flow.preview, size: flow.originalSize, crop: $flow.crop, turn: $flow.turn) }
        .confirmationDialog("Discard this photo?", isPresented: $discarding, titleVisibility: .visible) {
            Button("Discard", role: .destructive) { flow.retake() }
        } message: {
            Text("It hasn't been published. You can take another one.")
        }
    }

    /// What editing does, or what was edited.
    private var editNote: String {
        let cropped = !flow.crop.isWhole(in: flow.originalSize)
        let turned = flow.turn != .zero
        switch (cropped, turned) {
        case (true, true): return "Cropped and turned \(flow.turn.degrees)°. Only this part gets published."
        case (true, false): return "Cropped. Only this part gets published."
        case (false, true): return "Turned \(flow.turn.degrees)°."
        case (false, false): return "Tap the photo to crop or turn it."
        }
    }
}

/// Step two: is this where you were, and how much of it to share. Publishing is the yes.
struct AreaScreen: View {
    @Bindable var flow: CaptureFlow
    @State private var notHere = false

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                CloseButton(label: "Back", icon: "chevron.left") { withAnimation(Theme.ease) { flow.photoChosen = false } }
                Spacer()
            }
            .padding(.horizontal, 16)
            .frame(height: 56)
            VStack(alignment: .leading, spacing: 20) {
                Heading(title: "Were you around here?", subtitle: "People see this area, not where you stood.")
                ZoomAreaMap(flow: flow, you: you)
                    .frame(minHeight: 220)
                AreaSize(flow: flow)
                if flow.cellFailed {
                    Callout(tone: .danger, title: "Couldn't place this area", message: "Zoom the map to try another size.")
                        .transition(.opacity.combined(with: .offset(y: 6)))
                } else if flow.looseFix {
                    Callout(tone: .warn, title: "Your location was rough", message: "A larger area is safer.")
                        .transition(.opacity.combined(with: .offset(y: 6)))
                }
            }
            .padding(.horizontal, 24)
            .padding(.top, 8)
            .padding(.bottom, 24)
            HStack(spacing: 10) {
                Button("Not quite") { notHere = true }.buttonStyle(.ghost)
                    .frame(maxWidth: 140)
                Button {
                    flow.publish()
                } label: {
                    if flow.cellPending {
                        Spinner(color: Theme.onInk).frame(width: 16, height: 16)
                    } else {
                        Text("Yes, publish")
                    }
                }
                .buttonStyle(.primary(busy: flow.cellPending))
                .disabled(!flow.canPublish)
            }
            .padding(.horizontal, 24)
            .padding(.bottom, 12)
        }
        .background(Theme.paper.ignoresSafeArea())
        .animation(Theme.ease, value: flow.cellFailed)
        .animation(Theme.ease, value: flow.looseFix)
        .sheet(isPresented: $notHere) { NotHereSheet(flow: flow) }
    }

    private var you: CLLocationCoordinate2D? {
        flow.shot.map { CLLocationCoordinate2D(latitude: $0.fix.latitude, longitude: $0.fix.longitude) }
    }
}

/// The area on a map that sets the size: pinch in for a smaller area, out
/// for a larger one. The size follows the zoom live, and letting go snaps
/// the map to frame the area.
private struct ZoomAreaMap: View {
    @Bindable var flow: CaptureFlow
    let you: CLLocationCoordinate2D?
    @State private var camera = MapCameraPosition.automatic
    @State private var pinching = false
    /// Set while the map moves on its own, so that motion isn't read as a pinch.
    @State private var fitting = false
    @State private var fitID = 0
    @State private var hinted = false

    /// The cell's width against the shorter side of the map once framed.
    private static let fill = 0.55

    var body: some View {
        Map(position: $camera, bounds: MapCameraBounds(minimumDistance: 350, maximumDistance: 300_000), interactionModes: [.zoom]) {
            if let outline = flow.area {
                MapPolygon(coordinates: AreaMap.coordinates(outline))
                    .foregroundStyle(Theme.signalBright.opacity(pinching ? 0.22 : 0.16))
                    .stroke(Theme.signal, style: StrokeStyle(lineWidth: 2.5, lineJoin: .round))
            }
        }
        .mapStyle(.standard(emphasis: .muted, pointsOfInterest: .excludingAll))
        .mapControls {}
        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.md))
        .overlay(RoundedRectangle(cornerRadius: Theme.Radius.md).strokeBorder(pinching ? Theme.signal.opacity(0.5) : Theme.line))
        .overlay(alignment: .top) {
            if !hinted {
                Label("Pinch to change the size", systemImage: "hand.pinch")
                    .font(Typeface.sans(13.5, .semibold, relativeTo: .footnote))
                    .foregroundStyle(Theme.ink)
                    .padding(.horizontal, 14)
                    .frame(height: 34)
                    .background(.regularMaterial, in: Capsule())
                    .padding(.top, 14)
                    .transition(.opacity.combined(with: .offset(y: -8)))
                    .allowsHitTesting(false)
            }
        }
        .onMapCameraChange(frequency: .continuous) { context in
            guard !fitting else { return }
            // Framing the area never changes the size, so a change means a pinch.
            let resolution = Self.resolution(for: context.region)
            guard pinching || resolution != flow.resolution else { return }
            if !pinching { withAnimation(Theme.ease) { pinching = true; hinted = true } }
            if resolution != flow.resolution { flow.resolution = resolution }
        }
        .onMapCameraChange(frequency: .onEnd) { _ in
            if fitting {
                fitting = false
                return
            }
            withAnimation(Theme.ease) { pinching = false }
            fit()
        }
        .onChange(of: flow.area?.cell, initial: true) { _, _ in
            if !pinching { fit() }
        }
        .sensoryFeedback(.selection, trigger: flow.resolution)
        .onChange(of: flow.resolution) { _, _ in Sound.tick.play() }
        .accessibilityElement()
        .accessibilityLabel("Shared area")
        .accessibilityValue(AreaWords.names[flow.resolution] ?? "")
        .accessibilityHint("Swipe up or down to share a smaller or larger area.")
        .accessibilityAdjustableAction { direction in
            switch direction {
            case .increment: if flow.resolution < Verdict.resolutions.upperBound { flow.resolution += 1 }
            case .decrement: if flow.resolution > Verdict.resolutions.lowerBound { flow.resolution -= 1 }
            @unknown default: break
            }
        }
    }

    /// The size whose cell fills the visible map the way a framed area does.
    static func resolution(for region: MKCoordinateRegion) -> Int {
        let tall = region.span.latitudeDelta * 111_000
        let wide = region.span.longitudeDelta * 111_000 * cos(region.center.latitude * .pi / 180)
        let side = max(min(tall, wide), 1)
        return Verdict.resolutions.min { a, b in
            abs(log(width(a) / side / fill)) < abs(log(width(b) / side / fill))
        } ?? Verdict.defaultResolution
    }

    private static func width(_ resolution: Int) -> Double {
        Verdict.averageKm2[resolution].squareRoot() * 1000
    }

    private func fit() {
        let target: MapCameraPosition
        if let outline = flow.area {
            let points = AreaMap.coordinates(outline).map(MKMapPoint.init)
            guard let first = points.first else { return }
            var rect = MKMapRect(origin: first, size: MKMapSize(width: 0, height: 0))
            for point in points.dropFirst() { rect = rect.union(MKMapRect(origin: point, size: MKMapSize(width: 0, height: 0))) }
            let pad = max(rect.width, rect.height) * 0.22
            target = .rect(rect.insetBy(dx: -pad, dy: -pad))
        } else if let you {
            let side = Self.width(flow.resolution) / Self.fill
            target = .region(MKCoordinateRegion(center: you, latitudinalMeters: side, longitudinalMeters: side))
        } else {
            return
        }
        fitting = true
        fitID += 1
        let mine = fitID
        withAnimation(.spring(duration: 0.45, bounce: 0.12)) { camera = target }
        // If the map was already there, no end event comes to clear the flag.
        Task {
            try? await Task.sleep(for: .seconds(1))
            if mine == fitID { fitting = false }
        }
    }
}

/// The chosen size in words, under the map.
private struct AreaSize: View {
    let flow: CaptureFlow
    @State private var previous = Verdict.defaultResolution

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            Text(AreaWords.names[flow.resolution] ?? "")
                .font(Typeface.headline)
                .foregroundStyle(Theme.ink)
                .contentTransition(.numericText(countsDown: flow.resolution < previous))
            Spacer()
            Text("about \(Verdict.area(Verdict.averageKm2[flow.resolution]))")
                .font(Typeface.sans(15, relativeTo: .subheadline))
                .foregroundStyle(Theme.soft)
                .monospacedDigit()
                .contentTransition(.numericText(countsDown: flow.resolution < previous))
        }
        .animation(.snappy(duration: 0.25), value: flow.resolution)
        .onChange(of: flow.resolution) { old, _ in previous = old }
        .accessibilityHidden(true)
    }
}

/// "Not quite": the place came from the phone, so the fixes are a larger area or a new photo.
private struct NotHereSheet: View {
    let flow: CaptureFlow
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            Heading(
                title: "Not where you were?",
                subtitle: "Share a larger area that includes where you were, or take the photo again outside.")
            Spacer(minLength: 0)
            VStack(spacing: 10) {
                if flow.resolution > Verdict.resolutions.lowerBound {
                    Button("Share a larger area") {
                        flow.resolution -= 1
                        dismiss()
                    }
                    .buttonStyle(.primary)
                }
                Button("Take it again") {
                    dismiss()
                    flow.retake()
                }
                .buttonStyle(flow.resolution > Verdict.resolutions.lowerBound ? PillButtonStyle(kind: .ghost) : PillButtonStyle(kind: .primary))
            }
        }
        .padding(24)
        .background(Theme.paper.ignoresSafeArea())
        .presentationDetents([.medium])
        .presentationCornerRadius(Theme.Radius.lg)
    }
}

import CoreLocation
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
                VStack(spacing: 28) {
                    ApertureLoader(size: 88)
                    Text("Certifying your photo").font(Typeface.headline).foregroundStyle(Theme.ink)
                }
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

    var body: some View {
        VStack(spacing: 20) {
            Image(systemName: "exclamationmark").font(.system(size: 24, weight: .bold)).foregroundStyle(Theme.danger)
                .frame(width: 64, height: 64).background(Theme.dangerSoft, in: Circle())
                .accessibilityHidden(true)
            Heading(title: title, subtitle: message, center: true)
        }
        .padding(.horizontal, 32)
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
            Spacer()
            VStack(alignment: .leading, spacing: 24) {
                Heading(title: "Your photo is certified")
                PublishedImage(image: flow.preview, crop: flow.crop)
                    .frame(maxWidth: .infinity)
                    .onTapGesture { cropping = true }
                    .accessibilityAddTraits(.isButton)
                    .accessibilityHint("Edit the photo")
            }
            .padding(.horizontal, 24)
            Spacer()
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
        .fullScreenCover(isPresented: $cropping) { CropEditor(image: flow.preview, crop: $flow.crop) }
        .confirmationDialog("Discard this photo?", isPresented: $discarding, titleVisibility: .visible) {
            Button("Discard", role: .destructive) { flow.retake() }
        } message: {
            Text("It hasn't been published. You can take another one.")
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
                Heading(title: "Were you around here?", subtitle: "This is all people will see. Your location stays private.")
                AreaMap(outline: flow.area, you: you, span: flow.cellWidth * 3, height: nil)
                    .frame(minHeight: 220)
                ScrollViewReader { reader in
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 8) {
                            ForEach(Verdict.resolutions.reversed(), id: \.self) { resolution in
                                Chip(title: AreaWords.names[resolution] ?? "", selected: flow.resolution == resolution) {
                                    withAnimation(Theme.ease) { flow.resolution = resolution }
                                }
                                .id(resolution)
                            }
                        }
                    }
                    .contentMargins(.horizontal, 24, for: .scrollContent)
                    .padding(.horizontal, -24)
                    .onAppear { reader.scrollTo(flow.resolution, anchor: .center) }
                    .accessibilityLabel("How much to share")
                }
                if flow.cellFailed {
                    Callout(tone: .danger, title: "Couldn't place this area", message: "Pick another size.")
                } else if flow.looseFix {
                    Callout(tone: .warn, title: "Your location was rough", message: "A larger area is safer.")
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
        .sheet(isPresented: $notHere) { NotHereSheet(flow: flow) }
    }

    private var you: CLLocationCoordinate2D? {
        flow.shot.map { CLLocationCoordinate2D(latitude: $0.fix.latitude, longitude: $0.fix.longitude) }
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

/// The editor. For now it chooses the part of the photo to publish: drag a
/// corner to resize, drag inside to move, or pick a preset.
struct CropEditor: View {
    let image: CGImage?
    @Binding var crop: CropRect
    @State private var draft = CropRect.whole
    @Environment(\.dismiss) private var dismiss

    private static let presets: [(String, CropRect)] = [
        ("Whole", .whole),
        ("Left", CropPreset.leftHalf),
        ("Middle", CropRect(x: 256, y: 0, width: 512, height: 512)),
        ("Right", CropRect(x: 512, y: 0, width: 512, height: 512)),
    ]

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Button("Cancel") { dismiss() }
                Spacer()
                Text("Edit").font(Typeface.headline)
                Spacer()
                Button("Done") {
                    crop = draft
                    dismiss()
                }
                .font(Typeface.sans(16, .bold, relativeTo: .headline))
            }
            .font(Typeface.sans(16, .medium, relativeTo: .body))
            .foregroundStyle(.white)
            .padding(.horizontal, 20)
            .frame(height: 56)
            Spacer()
            CropCanvas(image: image, crop: $draft)
                .padding(.horizontal, 20)
            Spacer()
            HStack(spacing: 8) {
                ForEach(Self.presets, id: \.0) { name, rect in
                    Chip(title: name, selected: draft == rect, dark: true) { withAnimation(Theme.ease) { draft = rect } }
                }
            }
            .padding(.bottom, 24)
        }
        .background(Color.black.ignoresSafeArea())
        .preferredColorScheme(.dark)
        .onAppear { draft = crop }
    }
}

/// The 2:1 photo with the crop box, its handles and the shade.
private struct CropCanvas: View {
    let image: CGImage?
    @Binding var crop: CropRect
    @State private var start: CropRect?

    /// Smallest side, in original pixels, that a handle can make.
    private static let minSide = 64

    enum Handle: CaseIterable { case topLeft, topRight, bottomLeft, bottomRight }

    var body: some View {
        GeometryReader { geometry in
            let scale = geometry.size.width / Double(Original.width)
            let box = CGRect(
                x: Double(crop.x) * scale, y: Double(crop.y) * scale,
                width: Double(crop.width) * scale, height: Double(crop.height) * scale)
            ZStack(alignment: .topLeading) {
                CropPreview(image: image, crop: crop)
                Rectangle().strokeBorder(.white, lineWidth: 1.5)
                    .frame(width: box.width, height: box.height)
                    .offset(x: box.minX, y: box.minY)
                    .contentShape(Rectangle())
                    .gesture(move(scale))
                ForEach(Handle.allCases, id: \.self) { handle in
                    HandleMark(handle: handle)
                        .position(point(handle, box))
                        .gesture(resize(handle, scale))
                }
            }
        }
        .aspectRatio(2, contentMode: .fit)
        .accessibilityElement()
        .accessibilityLabel(crop.isWhole ? "Crop: the whole photo" : "Crop: part of the photo")
    }

    private func point(_ handle: Handle, _ box: CGRect) -> CGPoint {
        switch handle {
        case .topLeft: CGPoint(x: box.minX, y: box.minY)
        case .topRight: CGPoint(x: box.maxX, y: box.minY)
        case .bottomLeft: CGPoint(x: box.minX, y: box.maxY)
        case .bottomRight: CGPoint(x: box.maxX, y: box.maxY)
        }
    }

    private func move(_ scale: Double) -> some Gesture {
        DragGesture(minimumDistance: 1)
            .onChanged { value in
                let from = start ?? crop
                start = from
                let dx = Int((value.translation.width / scale).rounded())
                let dy = Int((value.translation.height / scale).rounded())
                crop = CropRect(
                    x: clamp(from.x + dx, 0, Original.width - from.width), y: clamp(from.y + dy, 0, Original.height - from.height),
                    width: from.width, height: from.height)
            }
            .onEnded { _ in start = nil }
    }

    private func resize(_ handle: Handle, _ scale: Double) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { value in
                let from = start ?? crop
                start = from
                let dx = Int((value.translation.width / scale).rounded())
                let dy = Int((value.translation.height / scale).rounded())
                var left = from.x
                var top = from.y
                var right = from.x + from.width
                var bottom = from.y + from.height
                switch handle {
                case .topLeft:
                    left = clamp(left + dx, 0, right - Self.minSide)
                    top = clamp(top + dy, 0, bottom - Self.minSide)
                case .topRight:
                    right = clamp(right + dx, left + Self.minSide, Original.width)
                    top = clamp(top + dy, 0, bottom - Self.minSide)
                case .bottomLeft:
                    left = clamp(left + dx, 0, right - Self.minSide)
                    bottom = clamp(bottom + dy, top + Self.minSide, Original.height)
                case .bottomRight:
                    right = clamp(right + dx, left + Self.minSide, Original.width)
                    bottom = clamp(bottom + dy, top + Self.minSide, Original.height)
                }
                crop = CropRect(x: left, y: top, width: right - left, height: bottom - top)
            }
            .onEnded { _ in start = nil }
    }
}

/// A white corner bracket with a 44 pt touch target.
private struct HandleMark: View {
    let handle: CropCanvas.Handle

    var body: some View {
        Canvas { context, size in
            let arm: CGFloat = 18
            let center = CGPoint(x: size.width / 2, y: size.height / 2)
            let dx: CGFloat = handle == .topLeft || handle == .bottomLeft ? 1 : -1
            let dy: CGFloat = handle == .topLeft || handle == .topRight ? 1 : -1
            var path = Path()
            path.move(to: CGPoint(x: center.x, y: center.y + dy * arm))
            path.addLine(to: center)
            path.addLine(to: CGPoint(x: center.x + dx * arm, y: center.y))
            context.stroke(path, with: .color(.white), style: StrokeStyle(lineWidth: 4, lineCap: .round, lineJoin: .round))
        }
        .frame(width: 44, height: 44)
        .contentShape(Rectangle())
        .accessibilityHidden(true)
    }
}

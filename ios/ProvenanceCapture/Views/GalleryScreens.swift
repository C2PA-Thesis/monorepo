import Photos
import PhotosUI
import SwiftUI

/// The verified photos on this iPhone, and a way to check any photo.
struct GalleryScreen: View {
    let flow: CaptureFlow
    @Environment(\.dismiss) private var dismiss
    @State private var path: [Route] = []
    @State private var picked: PhotosPickerItem?

    enum Route: Hashable {
        case photo(SavedPhoto)
        case check(Picked)
    }

    struct Picked: Hashable {
        let id = UUID()
        let data: Data
    }

    private var gallery: Gallery { flow.gallery }

    var body: some View {
        NavigationStack(path: $path) {
            VStack(alignment: .leading, spacing: 0) {
                HStack {
                    Text("Your photos").font(Typeface.sans(30, .bold, relativeTo: .largeTitle)).tight(30)
                        .foregroundStyle(Theme.ink)
                        .accessibilityAddTraits(.isHeader)
                    Spacer()
                    CloseButton(label: "Close") { dismiss() }
                }
                .padding(.horizontal, 24)
                .padding(.top, 28)
                .padding(.bottom, 20)
                if gallery.photos.isEmpty {
                    Spacer()
                    Heading(title: "No photos yet", subtitle: "Photos you publish show up here.", center: true)
                        .frame(maxWidth: .infinity)
                        .padding(.horizontal, 32)
                    Spacer()
                } else {
                    ScrollView {
                        LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 4), count: 3), spacing: 4) {
                            ForEach(gallery.photos) { photo in
                                NavigationLink(value: Route.photo(photo)) { Thumbnail(gallery: gallery, photo: photo) }
                                    .contextMenu {
                                        Button("Delete from this iPhone", systemImage: "trash", role: .destructive) {
                                            withAnimation(Theme.ease) { gallery.remove(photo) }
                                        }
                                    }
                            }
                        }
                        .padding(.horizontal, 24)
                    }
                }
                PhotosPicker(selection: $picked, matching: .images, preferredItemEncoding: .current) {
                    Label("Check a photo", systemImage: "checkmark.seal")
                }
                .buttonStyle(.ghost)
                .padding(.horizontal, 24)
                .padding(.top, 12)
                .padding(.bottom, 12)
            }
            .background(Theme.paper.ignoresSafeArea())
            .toolbar(.hidden, for: .navigationBar)
            .navigationDestination(for: Route.self) { route in
                switch route {
                case .photo(let photo):
                    SavedPhotoScreen(gallery: gallery, photo: photo, back: { path.removeLast() })
                case .check(let picked):
                    CheckScreen(flow: flow, file: picked.data, back: { path.removeLast() })
                }
            }
        }
        .presentationCornerRadius(Theme.Radius.lg)
        .onChange(of: picked) { _, item in
            guard let item else { return }
            Task {
                // The file as it is stored: a re-encoded copy would lose its proof.
                if let data = try? await item.loadTransferable(type: Data.self) { path.append(.check(Picked(data: data))) }
                picked = nil
            }
        }
    }
}

private struct Thumbnail: View {
    let gallery: Gallery
    let photo: SavedPhoto
    @State private var image: UIImage?

    var body: some View {
        Theme.surface
            .aspectRatio(1, contentMode: .fit)
            .overlay {
                if let image { Image(uiImage: image).resizable().scaledToFill() }
            }
            .clipShape(RoundedRectangle(cornerRadius: 8))
            .contentShape(RoundedRectangle(cornerRadius: 8))
            .task(id: photo.id) {
                guard let full = gallery.image(photo) else { return }
                let scale = 360 / min(full.size.width, full.size.height)
                image = await full.byPreparingThumbnail(ofSize: CGSize(width: full.size.width * scale, height: full.size.height * scale)) ?? full
            }
            .accessibilityLabel("Photo taken \(Verdict.when(photo.capturedAt))")
    }
}

/// One saved photo: verified, when it was taken, and what to do with the file.
private struct SavedPhotoScreen: View {
    let gallery: Gallery
    let photo: SavedPhoto
    let back: () -> Void
    @State private var deleting = false
    @State private var saved = false
    @State private var saveFailed = false

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                CloseButton(label: "Back", icon: "chevron.left", action: back)
                Spacer()
                CloseButton(label: "Delete", icon: "trash") { deleting = true }
            }
            .padding(.horizontal, 20)
            .padding(.top, 16)
            VerifiedView(image: gallery.image(photo), takenAt: photo.capturedAt)
            HStack(spacing: 10) {
                Button {
                    Task { await save() }
                } label: {
                    Label(saved ? "Saved" : "Save", systemImage: saved ? "checkmark" : "arrow.down.to.line")
                }
                .buttonStyle(.ghost)
                .frame(maxWidth: 140)
                .disabled(saved)
                ShareLink(item: gallery.file(photo)) { Label("Share", systemImage: "square.and.arrow.up") }
                    .buttonStyle(.primary)
            }
            .padding(.horizontal, 24)
            .padding(.bottom, 12)
        }
        .background(Theme.paper.ignoresSafeArea())
        .toolbar(.hidden, for: .navigationBar)
        .confirmationDialog("Delete this photo?", isPresented: $deleting, titleVisibility: .visible) {
            Button("Delete", role: .destructive) {
                gallery.remove(photo)
                back()
            }
        } message: {
            Text("It's removed from this iPhone. Copies you shared stay verified.")
        }
        .alert("Couldn't save it", isPresented: $saveFailed) {
            OpenSettingsButton(style: PillButtonStyle(kind: .primary))
            Button("OK", role: .cancel) {}
        } message: {
            Text("Allow Capture to add photos in Settings.")
        }
    }

    /// Adds the signed file itself, not a re-encoded image, so it keeps its proof.
    private func save() async {
        let status = await PHPhotoLibrary.requestAuthorization(for: .addOnly)
        guard status == .authorized || status == .limited else {
            saveFailed = true
            return
        }
        do {
            try await Self.addToLibrary(gallery.file(photo))
            withAnimation(Theme.ease) { saved = true }
        } catch {
            saveFailed = true
        }
    }
}

extension SavedPhotoScreen {
    /// Photos runs the change block on its own queue, so it must not inherit the main actor.
    nonisolated static func addToLibrary(_ url: URL) async throws {
        try await PHPhotoLibrary.shared().performChanges {
            PHAssetCreationRequest.forAsset().addResource(with: .photo, fileURL: url, options: nil)
        }
    }
}

/// Checks a photo picked from the library with the laptop's reader.
private struct CheckScreen: View {
    let flow: CaptureFlow
    let file: Data
    let back: () -> Void
    @State private var state = Checking.running
    @State private var popped = false

    enum Checking {
        case running
        case passed(Claim, Outline?)
        case failed(String)
        case unreachable
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                CloseButton(label: "Back", icon: "chevron.left", action: back)
                Spacer()
            }
            .padding(.horizontal, 20)
            .padding(.top, 16)
            switch state {
            case .running:
                Spacer()
                VStack(spacing: 28) {
                    ApertureLoader(size: 88)
                    Text("Checking this photo").font(Typeface.sans(20, .bold, relativeTo: .title3)).foregroundStyle(Theme.ink)
                }
                Spacer()
                Color.clear.frame(height: 68)
            case .passed(let claim, let outline):
                VStack(spacing: 28) {
                    VStack(spacing: 18) {
                        VerifiedMark(size: 72)
                            .scaleEffect(popped ? 1 : 0.4)
                            .opacity(popped ? 1 : 0)
                        Heading(title: "Verified", subtitle: Self.summary(claim), center: true)
                    }
                    AreaMap(outline: outline, height: nil).frame(minHeight: 220)
                }
                .padding(.horizontal, 24)
                .padding(.top, 24)
                .padding(.bottom, 24)
                .onAppear { withAnimation(.spring(duration: 0.6, bounce: 0.5).delay(0.15)) { popped = true } }
                Button("Done", action: back).buttonStyle(.primary).padding(.horizontal, 24).padding(.bottom, 12)
            case .failed(let message):
                Spacer()
                Problem(title: "Can't verify this photo", message: message)
                Spacer()
                Button("Done", action: back).buttonStyle(.primary).padding(.horizontal, 24).padding(.bottom, 12)
            case .unreachable:
                Spacer()
                Problem(title: "Your laptop didn't answer", message: "Check it's on and on the same Wi-Fi.")
                Spacer()
                VStack(spacing: 6) {
                    Button("Try again") { Task { await run() } }.buttonStyle(.primary)
                    Button("Done", action: back).buttonStyle(.quiet)
                }
                .padding(.horizontal, 24)
                .padding(.bottom, 12)
            }
        }
        .background(Theme.paper.ignoresSafeArea())
        .toolbar(.hidden, for: .navigationBar)
        .task { await run() }
    }

    private func run() async {
        state = .running
        guard let api = flow.api else {
            state = .unreachable
            return
        }
        do {
            let report = try await api.verify(file)
            if let claim = report.verdict.claim {
                state = .passed(claim, try? Native.library.outline(claim.cell))
            } else {
                state = .failed(Self.reason(report.verdict.rejected?.check))
            }
        } catch is HTTPError {
            state = .failed("Try again.")
        } catch {
            state = .unreachable
        }
    }

    /// The first check the file failed, said plainly.
    private static func reason(_ check: String?) -> String {
        switch check {
        case "device": "Your laptop doesn't know the iPhone that took it."
        case "image": "It was changed after it was taken."
        case "location": "Where it was taken doesn't check out."
        default: "It has no proof. It wasn't taken with Capture, or the proof was lost when it was saved or sent."
        }
    }

    private static func summary(_ claim: Claim) -> String {
        let today = ISO8601DateFormatter().date(from: claim.capturedAt).map(Calendar.current.isDateInToday) ?? false
        return "Taken \(today ? "today at " : "")\(Verdict.when(claim.capturedAt)), somewhere in this area."
    }
}

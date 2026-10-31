import SwiftUI

struct ContentView: View {
    @State private var flow = CaptureFlow()

    var body: some View {
        ScrollViewReader { scroll in
            ScrollView {
                VStack(spacing: 12) {
                    header
                    PairStep(flow: flow)
                    PhotoStep(flow: flow)
                    FrameStep(flow: flow)
                    RegionStep(flow: flow)
                    SendStep(flow: flow)
                    LaptopStep(flow: flow)
                    if let outcome = flow.outcome {
                        ResultCard(outcome: outcome, api: flow.api) {
                            flow.retake()
                            withAnimation { scroll.scrollTo("photo", anchor: .top) }
                        }
                        .id("result")
                    }
                    Text("Research prototype. The location proof is only as trustworthy as the device that signed it.")
                        .font(.caption).foregroundStyle(Theme.muted).multilineTextAlignment(.center)
                        .padding(.vertical, 12)
                }
                .padding(.horizontal, 16)
                .padding(.top, 8)
            }
            .onChange(of: flow.outcome != nil) { _, shown in
                if shown { withAnimation { scroll.scrollTo("result", anchor: .top) } }
            }
        }
        .background(Theme.bg.ignoresSafeArea())
        .tint(Theme.accent)
        .task { await flow.boot() }
        .fullScreenCover(isPresented: $flow.showScanner) {
            ScannerScreen { code in Task { await flow.scanned(code) } } cancel: { flow.showScanner = false }
        }
        .fullScreenCover(isPresented: $flow.showCamera) {
            CameraScreen { data, time in Task { await flow.photoTaken(data, at: time) } } cancel: { flow.showCamera = false }
        }
    }

    private var header: some View {
        HStack(spacing: 12) {
            Logo().frame(width: 30, height: 30).foregroundStyle(Theme.accent)
            VStack(alignment: .leading, spacing: 2) {
                Text("Private location capture").font(.title3.bold()).foregroundStyle(Theme.ink)
                Text("Publish part of a photo and the area it was taken in, with proofs")
                    .font(.footnote).foregroundStyle(Theme.muted)
            }
            Spacer(minLength: 0)
        }
        .padding(.vertical, 8)
    }
}

/// The hexagon with a dot from web/index.html.
struct Logo: View {
    var body: some View {
        Canvas { context, size in
            let scale = min(size.width, size.height) / 24
            var hexagon = Path()
            let points = [(12, 2.5), (20.2, 7.25), (20.2, 16.75), (12, 21.5), (3.8, 16.75), (3.8, 7.25)]
            hexagon.move(to: CGPoint(x: points[0].0 * scale, y: points[0].1 * scale))
            for point in points.dropFirst() { hexagon.addLine(to: CGPoint(x: point.0 * scale, y: point.1 * scale)) }
            hexagon.closeSubpath()
            context.stroke(hexagon, with: .foreground, style: StrokeStyle(lineWidth: 2 * scale, lineJoin: .round))
            let r = 2.6 * scale
            context.fill(Path(ellipseIn: CGRect(x: 12 * scale - r, y: 12 * scale - r, width: 2 * r, height: 2 * r)), with: .foreground)
        }
        .accessibilityHidden(true)
    }
}

#Preview {
    ContentView()
}

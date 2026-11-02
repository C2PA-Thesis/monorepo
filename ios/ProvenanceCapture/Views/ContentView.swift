import SwiftUI

struct ContentView: View {
    @State private var flow = CaptureFlow()

    var body: some View {
        ScrollViewReader { scroll in
            ScrollView {
                VStack(spacing: 12) {
                    header
                    StepStack(grouped: flow.outcome != nil) { steps }
                    if let outcome = flow.outcome {
                        ResultCard(outcome: outcome, outline: flow.outline) {
                            flow.retake()
                            withAnimation { scroll.scrollTo("photo", anchor: .top) }
                        }
                        .background(alignment: .top) { ScrollAnchor().id("result") }
                    }
                }
                .padding(.horizontal, 16)
                .padding(.top, 4)
                .padding(.bottom, 32)
            }
            .onChange(of: flow.outcome != nil) { _, shown in
                if shown { withAnimation { scroll.scrollTo("result", anchor: .top) } }
            }
        }
        // Scrolled content must not run under the status bar.
        .safeAreaInset(edge: .top, spacing: 0) { Color.clear.frame(height: 0).background(Theme.bg) }
        .background(Theme.bg.ignoresSafeArea())
        .tint(Theme.ink)
        .task { await flow.boot() }
        .fullScreenCover(isPresented: $flow.showScanner) {
            ScannerScreen { code in Task { await flow.scanned(code) } } cancel: { flow.showScanner = false }
        }
        .fullScreenCover(isPresented: $flow.showCamera) {
            CameraScreen { data, time in Task { await flow.photoTaken(data, at: time) } } cancel: { flow.showCamera = false }
        }
    }

    @ViewBuilder private var steps: some View {
        PairStep(flow: flow)
        PhotoStep(flow: flow)
        FrameStep(flow: flow)
        RegionStep(flow: flow)
        SendStep(flow: flow)
        LaptopStep(flow: flow)
    }

    private var header: some View {
        HStack(spacing: 10) {
            Logo().frame(width: 22, height: 22).foregroundStyle(Theme.ink)
            Text("Capture").font(.system(size: 17, weight: .semibold)).tracking(-0.17).foregroundStyle(Theme.ink)
            Spacer(minLength: 0)
        }
        // 16 pt to the first card with the stack's 12, as web/src/phone.css spaces it.
        .padding(.horizontal, 4)
        .padding(.top, 4)
        .padding(.bottom, 4)
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isHeader)
    }
}

/// A scroll target 12 pt above the view it backs, so the view stops clear of the top edge.
struct ScrollAnchor: View {
    var body: some View {
        Color.clear.frame(height: 12).alignmentGuide(.top) { $0[.bottom] }.accessibilityHidden(true)
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
            context.stroke(hexagon, with: .foreground, style: StrokeStyle(lineWidth: 1.8 * scale, lineJoin: .round))
            let r = 2.4 * scale
            context.fill(Path(ellipseIn: CGRect(x: 12 * scale - r, y: 12 * scale - r, width: 2 * r, height: 2 * r)), with: .foreground)
        }
        .accessibilityHidden(true)
    }
}

#Preview {
    ContentView()
}

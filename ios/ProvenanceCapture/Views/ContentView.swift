import SwiftUI

struct ContentView: View {
    @State private var flow = CaptureFlow()

    var body: some View {
        ZStack {
            switch flow.stage {
            case .launch: Theme.paper.ignoresSafeArea()
            case .welcome: WelcomeScreen(flow: flow)
            case .pair: PairScreen(flow: flow)
            case .location: LocationScreen(flow: flow)
            case .cameraAccess: CameraAccessScreen(flow: flow)
            case .camera: CameraHome(flow: flow)
            case .sealing: SealScreen(flow: flow)
            case .photo: PhotoScreen(flow: flow)
            case .area: AreaScreen(flow: flow)
            case .publishing: PublishScreen(flow: flow)
            case .result: ResultScreen(flow: flow)
            }
        }
        .transition(.opacity)
        .animation(Theme.ease, value: flow.stage)
        .tint(Theme.ink)
        .task {
            await flow.boot()
            #if DEBUG
                flow.applyScenario()
            #endif
        }
        .fullScreenCover(isPresented: $flow.showScanner) {
            ScannerScreen { code in Task { await flow.scanned(code) } } cancel: { flow.showScanner = false }
        }
    }
}

#Preview {
    ContentView()
}

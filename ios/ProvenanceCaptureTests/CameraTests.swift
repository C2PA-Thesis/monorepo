import XCTest

@testable import ProvenanceCapture

final class CameraTests: XCTestCase {
    /// Before this guard, the shutter on a camera that never started raised
    /// "No active and enabled video connection" and killed the app.
    func testShutterOnACameraThatNeverStartedThrows() async {
        do {
            _ = try await Camera().takePhoto()
            XCTFail("a camera that never started took a photo")
        } catch {
            XCTAssertNotNil(error as? CaptureError)
        }
    }
}

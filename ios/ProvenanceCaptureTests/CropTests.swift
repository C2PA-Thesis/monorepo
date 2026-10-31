import XCTest

@testable import ProvenanceCapture

final class CropTests: XCTestCase {
    func testPresetsMatchTheWebPage() {
        XCTAssertEqual(CropPreset.all.map(\.name), ["Left half", "Center", "Right half", "Whole frame"])
        XCTAssertEqual(CropPreset.all[1].rect, CropRect(x: 256, y: 0, width: 512, height: 512))
        XCTAssertEqual(CropPreset.all[3].rect, CropRect(x: 0, y: 0, width: 1024, height: 512))
    }

    func testDragInsideMovesAndStaysInTheFrame() {
        let drag = CropDrag(start: CropPoint(x: 100, y: 100), current: CropPreset.leftHalf)
        XCTAssertNotNil(drag.moving)
        XCTAssertEqual(drag.rect(to: CropPoint(x: 300, y: 100)), CropRect(x: 200, y: 0, width: 512, height: 512))
        XCTAssertEqual(drag.rect(to: CropPoint(x: 1000, y: 400)), CropRect(x: 512, y: 0, width: 512, height: 512))
        XCTAssertEqual(drag.rect(to: CropPoint(x: 0, y: 0)), CropRect(x: 0, y: 0, width: 512, height: 512))
    }

    func testDragOutsideDrawsANewBox() {
        let drag = CropDrag(start: CropPoint(x: 900, y: 400), current: CropPreset.leftHalf)
        XCTAssertNil(drag.moving)
        XCTAssertEqual(drag.rect(to: CropPoint(x: 700, y: 100)), CropRect(x: 700, y: 100, width: 200, height: 300))
        // Never smaller than 16 pixels, never past the edge.
        XCTAssertEqual(drag.rect(to: CropPoint(x: 901, y: 401)), CropRect(x: 900, y: 400, width: 16, height: 16))
        let corner = CropDrag(start: CropPoint(x: 1024, y: 512), current: CropPreset.leftHalf)
        XCTAssertEqual(corner.rect(to: CropPoint(x: 1024, y: 512)), CropRect(x: 1008, y: 496, width: 16, height: 16))
    }

    func testViewPointsMapToOriginalPixels() {
        XCTAssertEqual(CropDrag.originalPoint(x: 160, y: 80, viewWidth: 320), CropPoint(x: 512, y: 256))
        XCTAssertEqual(CropDrag.originalPoint(x: -5, y: 500, viewWidth: 320), CropPoint(x: 0, y: 512))
        XCTAssertEqual(CropDrag.originalPoint(x: 0.15, y: 0, viewWidth: 320), CropPoint(x: 0, y: 0))
    }

    func testFramingWindow() {
        let wide = FramingWindow(photoWidth: 4000, photoHeight: 1000)
        XCTAssertTrue(wide.wide)
        XCTAssertEqual(wide.width, 2000)
        XCTAssertEqual(wide.height, 1000)
        XCTAssertTrue(wide.slidable)
        XCTAssertEqual(wide.origin(at: 0.5).x, 1000)
        XCTAssertEqual(wide.origin(at: 1).x, 2000)

        let tall = FramingWindow(photoWidth: 1024, photoHeight: 683)
        XCTAssertFalse(tall.wide)
        XCTAssertEqual(tall.height, 512)
        XCTAssertEqual(tall.origin(at: 0.5).y, 85.5)
        XCTAssertTrue(tall.slidable)

        XCTAssertFalse(FramingWindow(photoWidth: 2048, photoHeight: 1024).slidable)
    }

    func testRenderedOriginalIsOpaque1024x512() throws {
        let url = try XCTUnwrap(Bundle(for: CaptureFlow.self).url(forResource: "sample", withExtension: "jpg"))
        let photo = try Photo(data: Data(contentsOf: url))
        let original = try OriginalImage.render(photo, position: 0.5)
        XCTAssertEqual(original.image.width, 1024)
        XCTAssertEqual(original.image.height, 512)
        let planes = original.channels()
        XCTAssertEqual(planes.r.count, Original.pixels)
        XCTAssertEqual(original.rgbx[0], planes.r[0])
        let png = try original.png()
        let decoded = try Photo(data: png)
        XCTAssertEqual(decoded.image.width, 1024)
        // The laptop fingerprints the PNG's pixels, so they must be the buffer's.
        let again = try OriginalImage.render(decoded, position: 0.5)
        XCTAssertEqual(again.rgbx, original.rgbx)
    }
}

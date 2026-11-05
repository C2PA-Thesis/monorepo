import XCTest

@testable import ProvenanceCapture

final class CropTests: XCTestCase {
    private func assertInside(_ rect: CropRect, file: StaticString = #filePath, line: UInt = #line) {
        // What the crop proof accepts (crates/crop-proof, Rect::check).
        XCTAssertTrue(rect.width > 0 && rect.height > 0, file: file, line: line)
        XCTAssertTrue(rect.x >= 0 && rect.y >= 0, file: file, line: line)
        XCTAssertLessThanOrEqual(rect.x + rect.width, Original.width, file: file, line: line)
        XCTAssertLessThanOrEqual(rect.y + rect.height, Original.height, file: file, line: line)
    }

    func testMoveStaysInTheFrame() {
        let half = CropRect(x: 0, y: 0, width: 512, height: 512)
        XCTAssertEqual(half.moved(dx: 200, dy: 0), CropRect(x: 200, y: 0, width: 512, height: 512))
        XCTAssertEqual(half.moved(dx: 900, dy: 300), CropRect(x: 512, y: 0, width: 512, height: 512))
        XCTAssertEqual(half.moved(dx: -50, dy: -50), half)
    }

    func testFreeHandlesMoveTheirOwnEdges() {
        let whole = CropRect.whole
        XCTAssertEqual(whole.resized(.topLeft, dx: 100, dy: 50), CropRect(x: 100, y: 50, width: 924, height: 462))
        XCTAssertEqual(whole.resized(.right, dx: -300, dy: 80), CropRect(x: 0, y: 0, width: 724, height: 512))
        XCTAssertEqual(whole.resized(.bottom, dx: 40, dy: -112), CropRect(x: 0, y: 0, width: 1024, height: 400))
        // Never past the edge, never smaller than the minimum.
        XCTAssertEqual(whole.resized(.bottomRight, dx: 50, dy: 50), whole)
        XCTAssertEqual(whole.resized(.left, dx: 2000, dy: 0).width, CropRect.minSide)
    }

    func testFixedShapeKeepsItsRatioAndTheOppositeCorner() {
        let square = CropRect(x: 256, y: 0, width: 512, height: 512)
        let smaller = square.resized(corner: .topLeft, dx: 100, dy: 100, ratio: 1)
        XCTAssertEqual(smaller, CropRect(x: 356, y: 100, width: 412, height: 412))
        // Growing stops where the photo ends.
        let wide = CropRect(x: 100, y: 100, width: 400, height: 300)
        let grown = wide.resized(corner: .bottomRight, dx: 900, dy: 900, ratio: 4.0 / 3)
        XCTAssertEqual(grown.x, 100)
        XCTAssertEqual(grown.y, 100)
        XCTAssertEqual(Double(grown.width) / Double(grown.height), 4.0 / 3, accuracy: 0.01)
        assertInside(grown)
    }

    func testEveryResultFitsThePhoto() {
        let ratios: [Double] = [1, 4.0 / 3, 16.0 / 9]
        let starts = [CropRect.whole, CropRect(x: 731, y: 213, width: 293, height: 299), CropRect(x: 0, y: 1, width: 65, height: 511)]
        for ratio in ratios {
            for start in starts {
                assertInside(.fit(ratio, around: start))
                for corner in CropHandle.allCases where corner.isCorner {
                    for d in [-2000, -333, -1, 0, 1, 333, 2000] {
                        assertInside(start.resized(corner: corner, dx: d, dy: -d, ratio: ratio))
                    }
                }
            }
        }
        for start in starts {
            for handle in CropHandle.allCases {
                for d in [-2000, -1, 1, 2000] { assertInside(start.resized(handle, dx: d, dy: d)) }
            }
        }
    }

    func testFitIsTheLargestBoxOfThatShape() {
        XCTAssertEqual(CropRect.fit(1, around: .whole), CropRect(x: 256, y: 0, width: 512, height: 512))
        let wide = CropRect.fit(16.0 / 9, around: CropRect(x: 900, y: 0, width: 124, height: 512))
        XCTAssertEqual(wide, CropRect(x: 114, y: 1, width: 910, height: 511))
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

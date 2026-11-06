import XCTest

@testable import ProvenanceCapture

final class CropTests: XCTestCase {
    /// The handle tests run on a 2:1 original, as before the size became an input.
    private let size = OriginalSize(width: 1024, height: 512)

    private func assertInside(_ rect: CropRect, file: StaticString = #filePath, line: UInt = #line) {
        // What the crop proof accepts (crates/crop-proof, Rect::check).
        XCTAssertTrue(rect.width > 0 && rect.height > 0, file: file, line: line)
        XCTAssertTrue(rect.x >= 0 && rect.y >= 0, file: file, line: line)
        XCTAssertLessThanOrEqual(rect.x + rect.width, size.width, file: file, line: line)
        XCTAssertLessThanOrEqual(rect.y + rect.height, size.height, file: file, line: line)
    }

    func testMoveStaysInTheFrame() {
        let half = CropRect(x: 0, y: 0, width: 512, height: 512)
        XCTAssertEqual(half.moved(dx: 200, dy: 0, in: size), CropRect(x: 200, y: 0, width: 512, height: 512))
        XCTAssertEqual(half.moved(dx: 900, dy: 300, in: size), CropRect(x: 512, y: 0, width: 512, height: 512))
        XCTAssertEqual(half.moved(dx: -50, dy: -50, in: size), half)
    }

    func testFreeHandlesMoveTheirOwnEdges() {
        let whole = CropRect.whole(size)
        XCTAssertEqual(whole.resized(.topLeft, dx: 100, dy: 50, in: size), CropRect(x: 100, y: 50, width: 924, height: 462))
        XCTAssertEqual(whole.resized(.right, dx: -300, dy: 80, in: size), CropRect(x: 0, y: 0, width: 724, height: 512))
        XCTAssertEqual(whole.resized(.bottom, dx: 40, dy: -112, in: size), CropRect(x: 0, y: 0, width: 1024, height: 400))
        // Never past the edge, never smaller than the minimum.
        XCTAssertEqual(whole.resized(.bottomRight, dx: 50, dy: 50, in: size), whole)
        XCTAssertEqual(whole.resized(.left, dx: 2000, dy: 0, in: size).width, CropRect.minSide)
    }

    func testFixedShapeKeepsItsRatioAndTheOppositeCorner() {
        let square = CropRect(x: 256, y: 0, width: 512, height: 512)
        let smaller = square.resized(corner: .topLeft, dx: 100, dy: 100, ratio: 1, in: size)
        XCTAssertEqual(smaller, CropRect(x: 356, y: 100, width: 412, height: 412))
        // Growing stops where the photo ends.
        let wide = CropRect(x: 100, y: 100, width: 400, height: 300)
        let grown = wide.resized(corner: .bottomRight, dx: 900, dy: 900, ratio: 4.0 / 3, in: size)
        XCTAssertEqual(grown.x, 100)
        XCTAssertEqual(grown.y, 100)
        XCTAssertEqual(Double(grown.width) / Double(grown.height), 4.0 / 3, accuracy: 0.01)
        assertInside(grown)
    }

    func testEveryResultFitsThePhoto() {
        let ratios: [Double] = [1, 4.0 / 3, 16.0 / 9]
        let starts = [CropRect.whole(size), CropRect(x: 731, y: 213, width: 293, height: 299), CropRect(x: 0, y: 1, width: 65, height: 511)]
        for ratio in ratios {
            for start in starts {
                assertInside(.fit(ratio, around: start, in: size))
                for corner in CropHandle.allCases where corner.isCorner {
                    for d in [-2000, -333, -1, 0, 1, 333, 2000] {
                        assertInside(start.resized(corner: corner, dx: d, dy: -d, ratio: ratio, in: size))
                    }
                }
            }
        }
        for start in starts {
            for handle in CropHandle.allCases {
                for d in [-2000, -1, 1, 2000] { assertInside(start.resized(handle, dx: d, dy: d, in: size)) }
            }
        }
    }

    func testFitIsTheLargestBoxOfThatShape() {
        XCTAssertEqual(CropRect.fit(1, around: .whole(size), in: size), CropRect(x: 256, y: 0, width: 512, height: 512))
        let wide = CropRect.fit(16.0 / 9, around: CropRect(x: 900, y: 0, width: 124, height: 512), in: size)
        XCTAssertEqual(wide, CropRect(x: 114, y: 1, width: 910, height: 511))
    }

    /// The values crates/fingerprint/src/image.rs pins for Size::fit.
    func testFitMatchesTheRustSide() {
        let cases: [(Int, Int, Int, Int)] = [
            (4032, 3024, 836, 627), (3024, 4032, 627, 836), (8064, 6048, 836, 627),
            (1920, 1080, 965, 543), (1024, 512, 1024, 512), (640, 480, 640, 480),
        ]
        for (width, height, fitWidth, fitHeight) in cases {
            XCTAssertEqual(
                OriginalSize.fit(width: width, height: height), OriginalSize(width: fitWidth, height: fitHeight),
                "fit(\(width), \(height))")
        }
        XCTAssertLessThanOrEqual(OriginalSize.fit(width: 100_000, height: 3).pixels, OriginalSize.maxPixels)
    }

    func testHandlesStayInsideATallOriginal() {
        let tall = OriginalSize(width: 627, height: 836)
        let whole = CropRect.whole(tall)
        XCTAssertEqual(whole.resized(.bottomRight, dx: 50, dy: 50, in: tall), whole)
        XCTAssertEqual(whole.moved(dx: 10, dy: 10, in: tall), whole)
        XCTAssertEqual(CropRect.fit(1, around: whole, in: tall), CropRect(x: 0, y: 105, width: 627, height: 627))
        XCTAssertTrue(whole.isWhole(in: tall))
        XCTAssertFalse(whole.isWhole(in: size))
    }

    func testRenderedOriginalIsTheWholePhotoOpaque() throws {
        let url = try XCTUnwrap(Bundle(for: CaptureFlow.self).url(forResource: "sample", withExtension: "jpg"))
        let photo = try Photo(data: Data(contentsOf: url))
        XCTAssertEqual(photo.image.width, 1024)
        XCTAssertEqual(photo.image.height, 683)
        let original = try OriginalImage.render(photo)
        XCTAssertEqual(original.size, OriginalSize(width: 886, height: 591))
        XCTAssertEqual(original.image.width, 886)
        XCTAssertEqual(original.image.height, 591)
        XCTAssertEqual(original.image.alphaInfo, .noneSkipLast)
        let planes = original.channels()
        XCTAssertEqual(planes.r.count, 886 * 591)
        XCTAssertEqual(original.rgbx[0], planes.r[0])
        let png = try original.png()
        let decoded = try Photo(data: png)
        XCTAssertEqual(decoded.image.width, 886)
        XCTAssertEqual(decoded.image.height, 591)
        // The laptop fingerprints the PNG's pixels, so they must be the buffer's.
        let again = try OriginalImage.render(decoded)
        XCTAssertEqual(again.rgbx, original.rgbx)
    }
}

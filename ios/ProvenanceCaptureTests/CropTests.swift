import SwiftUI
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

    func testTurnCyclesThroughFourQuarters() {
        XCTAssertEqual([Turn.zero, .zero.next, .zero.next.next, .zero.next.next.next].map(\.quarters), [0, 1, 2, 3])
        XCTAssertEqual(Turn.zero.next.next.next.next, .zero)
        XCTAssertEqual(Turn.zero.previous, Turn(quarters: 3))
        XCTAssertEqual(Turn(quarters: 2).previous.next, Turn(quarters: 2))
        XCTAssertEqual(Turn(quarters: -1), Turn(quarters: 3))
        XCTAssertEqual(Turn(quarters: 5), Turn(quarters: 1))
        XCTAssertEqual(Turn(quarters: 3).degrees, 270)
    }

    func testOddTurnsSwapTheSides() {
        let crop = CropRect(x: 10, y: 20, width: 300, height: 200)
        for quarters in 0..<4 {
            let size = crop.size(turned: Turn(quarters: quarters))
            if quarters % 2 == 0 {
                XCTAssertEqual([size.width, size.height], [300, 200], "\(quarters)")
            } else {
                XCTAssertEqual([size.width, size.height], [200, 300], "\(quarters)")
            }
        }
    }

    /// Turn::source in crates/crop-proof on a 3x2 image, whose pixel (x, y) is numbered y * 3 + x.
    func testSourceMatchesTheRustSide() {
        let expected: [Int: [Int]] = [
            0: [0, 1, 2, 3, 4, 5],
            1: [3, 0, 4, 1, 5, 2],
            2: [5, 4, 3, 2, 1, 0],
            3: [2, 5, 1, 4, 0, 3],
        ]
        for quarters in 0..<4 {
            let turn = Turn(quarters: quarters)
            let size = turn.size(width: 3, height: 2)
            var order: [Int] = []
            for v in 0..<size.height {
                for u in 0..<size.width {
                    let from = turn.source(u: u, v: v, width: 3, height: 2)
                    order.append(from.y * 3 + from.x)
                }
            }
            XCTAssertEqual(order, expected[quarters], "\(quarters)")
        }
    }

    /// The preview draws the turn the laptop applies: every pixel lands where Turn::source says.
    @MainActor
    func testPreviewOrientationDrawsTheTurn() throws {
        // Six colours that stay apart after any colour conversion.
        let colors: [[UInt8]] = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [0, 255, 255], [255, 0, 255], [255, 255, 0]]
        let source = try XCTUnwrap(Self.image(width: 3, height: 2, pixels: colors))
        for quarters in 0..<4 {
            let turn = Turn(quarters: quarters)
            let renderer = ImageRenderer(content: Image(decorative: source, scale: 1, orientation: turn.orientation))
            renderer.scale = 1
            let drawn = try XCTUnwrap(renderer.cgImage)
            let size = turn.size(width: 3, height: 2)
            XCTAssertEqual([drawn.width, drawn.height], [size.width, size.height], "\(quarters)")
            let pixels = try XCTUnwrap(Self.pixels(drawn))
            for v in 0..<size.height {
                for u in 0..<size.width {
                    let from = turn.source(u: u, v: v, width: 3, height: 2)
                    XCTAssertEqual(pixels[v * size.width + u], colors[from.y * 3 + from.x], "turn \(quarters) at (\(u), \(v))")
                }
            }
        }
    }

    func testUploadSendsTheTurnOnlyWhenThereIsOne() throws {
        let receipt = Receipt(
            fingerprint: Fingerprint(algorithm: "a", size: .init(width: 1, height: 1), r: [], g: [], b: []),
            envelope: "0x01", capturedAt: "t", device: "d", signature: "s")
        func encoded(_ turn: Turn) throws -> [String: Any] {
            let upload = CaptureUpload(
                originalPng: "png", receipt: receipt, cell: "87", resolution: 7, crop: CropRect(x: 0, y: 0, width: 512, height: 256),
                turn: turn, accuracyMeters: nil, locationProof: "proof", location: .init(simulated: true, accessory: false))
            return try XCTUnwrap(JSONSerialization.jsonObject(with: JSONEncoder().encode(upload)) as? [String: Any])
        }
        XCTAssertNil(try encoded(.zero)["turn"])
        for quarters in 1..<4 {
            let value = try XCTUnwrap(try encoded(Turn(quarters: quarters))["turn"] as? NSNumber)
            XCTAssertEqual(value.intValue, quarters)
            // A JSON number, not a string or a boolean.
            XCTAssertFalse(CFGetTypeID(value) == CFBooleanGetTypeID())
        }
        // The crop stays the rectangle in the original.
        let crop = try XCTUnwrap(try encoded(Turn(quarters: 1))["crop"] as? [String: Int])
        XCTAssertEqual(crop, ["x": 0, "y": 0, "width": 512, "height": 256])
    }

    func testTurnDecodesOnlyZeroToThree() throws {
        for quarters in 0..<4 {
            XCTAssertEqual(try JSONDecoder().decode(Turn.self, from: Data("\(quarters)".utf8)), Turn(quarters: quarters))
        }
        for bad in ["4", "-1", "\"1\"", "1.5"] {
            XCTAssertThrowsError(try JSONDecoder().decode(Turn.self, from: Data(bad.utf8)), bad)
        }
    }

    /// An sRGB RGBA image from RGB triples, row by row.
    private static func image(width: Int, height: Int, pixels: [[UInt8]]) -> CGImage? {
        let bytes: [UInt8] = pixels.flatMap { $0 + [UInt8(255)] }
        guard let provider = CGDataProvider(data: Data(bytes) as CFData) else { return nil }
        return CGImage(
            width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: width * 4,
            space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
            provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)
    }

    /// RGB triples, each channel rounded to 0 or 255.
    private static func pixels(_ image: CGImage) -> [[UInt8]]? {
        var bytes = [UInt8](repeating: 0, count: image.width * image.height * 4)
        let drawn = bytes.withUnsafeMutableBytes { buffer -> Bool in
            guard
                let context = CGContext(
                    data: buffer.baseAddress, width: image.width, height: image.height, bitsPerComponent: 8, bytesPerRow: image.width * 4,
                    space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)
            else { return false }
            context.interpolationQuality = .none
            context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
            return true
        }
        guard drawn else { return nil }
        return stride(from: 0, to: bytes.count, by: 4).map { index in
            bytes[index..<index + 3].map { $0 >= 128 ? 255 : 0 }
        }
    }
}

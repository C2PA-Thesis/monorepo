import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

/// A decoded photo, upright, as the camera or the picker handed it over.
struct Photo: @unchecked Sendable {
    // CGImage is immutable, so sharing it across tasks is safe.
    let image: CGImage

    var window: FramingWindow { FramingWindow(photoWidth: Double(image.width), photoHeight: Double(image.height)) }

    init(image: CGImage) {
        self.image = image
    }

    /// Decodes JPEG, HEIC or PNG bytes with the EXIF orientation applied, so a
    /// portrait photo comes in upright like it does in an <img>.
    init(data: Data) throws {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil),
            let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
            let width = properties[kCGImagePropertyPixelWidth] as? Int,
            let height = properties[kCGImagePropertyPixelHeight] as? Int
        else { throw CaptureError("the photo could not be decoded") }
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: max(width, height),
            kCGImageSourceShouldCacheImmediately: true,
        ]
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else {
            throw CaptureError("the photo could not be decoded")
        }
        self.image = image
    }
}

/// The 1024x512 original the device commits to: opaque 8-bit sRGB, so the
/// pixel buffer and its PNG describe the same bytes.
struct OriginalImage: @unchecked Sendable {
    // The buffer is never mutated after rendering.
    let rgbx: [UInt8]
    let image: CGImage

    static func render(_ photo: Photo, position: Double) throws -> OriginalImage {
        let width = Original.width
        let height = Original.height
        guard let space = CGColorSpace(name: CGColorSpace.sRGB),
            let context = CGContext(
                data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
                space: space, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)
        else { throw CaptureError("could not create the 1024×512 bitmap") }
        context.interpolationQuality = .high

        let window = photo.window
        let origin = window.origin(at: position)
        let scale = Double(width) / window.width
        // Core Graphics puts y = 0 at the bottom, the window's origin is top-left.
        let drawn = CGRect(
            x: -origin.x * scale,
            y: Double(height) + origin.y * scale - Double(photo.image.height) * scale,
            width: Double(photo.image.width) * scale,
            height: Double(photo.image.height) * scale)
        context.draw(photo.image, in: drawn)

        guard let data = context.data, let image = context.makeImage() else {
            throw CaptureError("could not read the 1024×512 bitmap")
        }
        let bytes = context.bytesPerRow * height
        let rgbx = [UInt8](UnsafeBufferPointer(start: data.assumingMemoryBound(to: UInt8.self), count: bytes))
        guard context.bytesPerRow == width * 4 else { throw CaptureError("unexpected bitmap row length") }
        return OriginalImage(rgbx: rgbx, image: image)
    }

    /// The red, green and blue planes, as the fingerprint takes them.
    func channels() -> (r: [UInt8], g: [UInt8], b: [UInt8]) {
        var r = [UInt8](repeating: 0, count: Original.pixels)
        var g = r
        var b = r
        rgbx.withUnsafeBufferPointer { pixels in
            for index in 0..<Original.pixels {
                r[index] = pixels[index * 4]
                g[index] = pixels[index * 4 + 1]
                b[index] = pixels[index * 4 + 2]
            }
        }
        return (r, g, b)
    }

    /// Lossless PNG of the same pixels.
    func png() throws -> Data {
        let out = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(out, UTType.png.identifier as CFString, 1, nil) else {
            throw CaptureError("PNG encoding failed")
        }
        CGImageDestinationAddImage(destination, image, nil)
        guard CGImageDestinationFinalize(destination) else { throw CaptureError("PNG encoding failed") }
        return out as Data
    }
}

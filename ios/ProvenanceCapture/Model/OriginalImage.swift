import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

/// A decoded photo, upright, as the camera or the picker handed it over.
struct Photo: @unchecked Sendable {
    // CGImage is immutable, so sharing it across tasks is safe.
    let image: CGImage

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

/// The original the device commits to: the whole photo at `OriginalSize.fit`,
/// opaque 8-bit sRGB, so the pixel buffer and its PNG describe the same bytes.
struct OriginalImage: @unchecked Sendable {
    // The buffer is never mutated after rendering.
    let rgbx: [UInt8]
    let image: CGImage

    var size: OriginalSize { OriginalSize(width: image.width, height: image.height) }

    static func render(_ photo: Photo) throws -> OriginalImage {
        let size = OriginalSize.fit(width: photo.image.width, height: photo.image.height)
        let width = size.width
        let height = size.height
        guard let space = CGColorSpace(name: CGColorSpace.sRGB),
            let context = CGContext(
                data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
                space: space, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)
        else { throw CaptureError("could not create the \(width)×\(height) bitmap") }
        context.interpolationQuality = .high
        context.draw(photo.image, in: CGRect(x: 0, y: 0, width: width, height: height))

        guard let data = context.data, let image = context.makeImage() else {
            throw CaptureError("could not read the \(width)×\(height) bitmap")
        }
        guard context.bytesPerRow == width * 4 else { throw CaptureError("unexpected bitmap row length") }
        let bytes = context.bytesPerRow * height
        let rgbx = [UInt8](UnsafeBufferPointer(start: data.assumingMemoryBound(to: UInt8.self), count: bytes))
        return OriginalImage(rgbx: rgbx, image: image)
    }

    /// The red, green and blue planes, as the fingerprint takes them.
    func channels() -> (r: [UInt8], g: [UInt8], b: [UInt8]) {
        let pixels = size.pixels
        var r = [UInt8](repeating: 0, count: pixels)
        var g = r
        var b = r
        rgbx.withUnsafeBufferPointer { rgbx in
            for index in 0..<pixels {
                r[index] = rgbx[index * 4]
                g[index] = rgbx[index * 4 + 1]
                b[index] = rgbx[index * 4 + 2]
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

import CoreGraphics
import ImageIO
import UIKit
import UniformTypeIdentifiers

enum ImageProcessing {
    struct Result {
        let data: Data
        let width: Int
        let height: Int
        let mime: String
    }

    enum Error: Swift.Error, LocalizedError {
        case noCGImage
        case encodingFailed

        var errorDescription: String? {
            switch self {
            case .noCGImage: return "Couldn't read the captured image."
            case .encodingFailed: return "Couldn't prepare the image for upload."
            }
        }
    }

    /// Re-encode `image` as JPEG, scaling its long edge to at most `maxEdge`
    /// and stripping all metadata (EXIF, GPS, TIFF, IPTC). The output is safe
    /// to upload publicly without leaking the user's location.
    static func stripMetadataAndEncode(
        _ image: UIImage,
        maxEdge: CGFloat = 2048,
        quality: CGFloat = 0.85
    ) throws -> Result {
        let scaled = image.resizedRespectingOrientation(maxEdge: maxEdge)
        guard let cgImage = scaled.cgImage else {
            throw Error.noCGImage
        }

        let buffer = NSMutableData()
        let type = UTType.jpeg.identifier as CFString
        guard let destination = CGImageDestinationCreateWithData(
            buffer as CFMutableData,
            type,
            1,
            nil
        ) else {
            throw Error.encodingFailed
        }

        // Pass an empty properties dictionary (plus quality) so the destination
        // does NOT copy the source's metadata. The kCGImageMetadata* keys are
        // the ones that would carry GPS/EXIF; omitting them is the strip.
        let properties: [CFString: Any] = [
            kCGImageDestinationLossyCompressionQuality: quality
        ]
        CGImageDestinationAddImage(destination, cgImage, properties as CFDictionary)
        guard CGImageDestinationFinalize(destination) else {
            throw Error.encodingFailed
        }

        return Result(
            data: buffer as Data,
            width: cgImage.width,
            height: cgImage.height,
            mime: "image/jpeg"
        )
    }

    /// Crop a processed JPEG to a page rect expressed in Vision normalized
    /// (bottom-left origin) coordinates and re-encode at high quality. Used
    /// by the post-OCR auto-crop that reduces a two-page spread down to the
    /// dominant page before upload.
    static func cropToPage(
        _ processed: Result,
        pageRect: CGRect,
        quality: CGFloat = 0.88
    ) throws -> Result {
        guard let provider = CGDataProvider(data: processed.data as CFData),
              let sourceImage = CGImage(
                jpegDataProviderSource: provider,
                decode: nil,
                shouldInterpolate: true,
                intent: .defaultIntent
              ) else {
            throw Error.noCGImage
        }

        let imageBounds = CGRect(
            x: 0,
            y: 0,
            width: CGFloat(sourceImage.width),
            height: CGFloat(sourceImage.height)
        )
        let pixelRect = CGRect(
            x: pageRect.minX * imageBounds.width,
            y: (1.0 - pageRect.maxY) * imageBounds.height,
            width: pageRect.width * imageBounds.width,
            height: pageRect.height * imageBounds.height
        ).intersection(imageBounds).integral

        guard !pixelRect.isNull, !pixelRect.isEmpty,
              let cropped = sourceImage.cropping(to: pixelRect) else {
            throw Error.noCGImage
        }
        let data = try encodeJPEG(cropped, quality: quality)
        return Result(
            data: data,
            width: cropped.width,
            height: cropped.height,
            mime: "image/jpeg"
        )
    }

    /// Build a normalized crop box around the selected OCR boxes. Coordinates
    /// use Vision's normalized bottom-left origin.
    static func defaultHighlightCropBox(
        highlightBoxes: [CGRect],
        imageSize: CGSize,
        marginFraction: CGFloat = 0.08
    ) -> CGRect? {
        guard let selectedBounds = highlightBoxes
            .filter({ !$0.isNull && !$0.isEmpty })
            .union()
        else {
            return nil
        }

        let marginX = max(marginFraction, 48 / max(imageSize.width, 1))
        let marginY = max(marginFraction, selectedBounds.height * 0.55, 48 / max(imageSize.height, 1))
        return selectedBounds
            .insetBy(dx: -marginX, dy: -marginY)
            .intersection(CGRect(x: 0, y: 0, width: 1, height: 1))
    }

    /// Crop the already-sanitized capture around the selected OCR boxes and
    /// bake the yellow highlight treatment into the pixels that get uploaded.
    static func cropAndAnnotateHighlight(
        _ processed: Result,
        highlightBoxes: [CGRect],
        cropBox: CGRect? = nil,
        marginFraction: CGFloat = 0.08,
        quality: CGFloat = 0.88
    ) throws -> Result {
        guard let provider = CGDataProvider(data: processed.data as CFData),
              let sourceImage = CGImage(
                jpegDataProviderSource: provider,
                decode: nil,
                shouldInterpolate: true,
                intent: .defaultIntent
              ) else {
            throw Error.noCGImage
        }

        let imageBounds = CGRect(
            x: 0,
            y: 0,
            width: CGFloat(sourceImage.width),
            height: CGFloat(sourceImage.height)
        )
        let pixelRects = highlightBoxes
            .map { visionToPixelRect($0, imageSize: imageBounds.size) }
            .filter { !$0.isNull && !$0.isEmpty }

        guard let selectedBounds = pixelRects.union() else {
            return processed
        }

        let cropRect: CGRect
        if let cropBox {
            cropRect = visionToPixelRect(cropBox, imageSize: imageBounds.size)
                .intersection(imageBounds)
                .integral
        } else {
            let marginX = max(imageBounds.width * marginFraction, 48)
            let marginY = max(imageBounds.height * marginFraction, selectedBounds.height * 0.55, 48)
            cropRect = selectedBounds
                .insetBy(dx: -marginX, dy: -marginY)
                .intersection(imageBounds)
                .integral
        }

        guard let croppedImage = sourceImage.cropping(to: cropRect) else {
            throw Error.noCGImage
        }

        let cropSize = CGSize(width: cropRect.width, height: cropRect.height)
        let format = UIGraphicsImageRendererFormat.default()
        format.scale = 1.0
        format.opaque = true

        let renderer = UIGraphicsImageRenderer(size: cropSize, format: format)
        let annotated = renderer.image { context in
            UIImage(cgImage: croppedImage).draw(in: CGRect(origin: .zero, size: cropSize))

            let cgContext = context.cgContext
            cgContext.setFillColor(UIColor.systemYellow.withAlphaComponent(0.25).cgColor)
            for rect in pixelRects {
                let local = rect.offsetBy(dx: -cropRect.minX, dy: -cropRect.minY)
                    .intersection(CGRect(origin: .zero, size: cropSize))
                guard !local.isNull, !local.isEmpty else { continue }
                cgContext.fill(local)
            }

            cgContext.setFillColor(UIColor.systemYellow.withAlphaComponent(0.85).cgColor)
            for rect in pixelRects {
                let local = rect.offsetBy(dx: -cropRect.minX, dy: -cropRect.minY)
                    .intersection(CGRect(origin: .zero, size: cropSize))
                guard !local.isNull, !local.isEmpty else { continue }
                let underlineHeight = max(4, min(8, local.height * 0.18))
                let underline = CGRect(
                    x: local.minX,
                    y: local.maxY - underlineHeight,
                    width: local.width,
                    height: underlineHeight
                )
                cgContext.fill(underline)
            }
        }

        guard let cgImage = annotated.cgImage else {
            throw Error.noCGImage
        }
        let data = try encodeJPEG(cgImage, quality: quality)
        return Result(
            data: data,
            width: cgImage.width,
            height: cgImage.height,
            mime: "image/jpeg"
        )
    }

    private static func visionToPixelRect(_ bbox: CGRect, imageSize: CGSize) -> CGRect {
        CGRect(
            x: bbox.minX * imageSize.width,
            y: (1.0 - bbox.maxY) * imageSize.height,
            width: bbox.width * imageSize.width,
            height: bbox.height * imageSize.height
        ).integral
    }

    private static func encodeJPEG(_ cgImage: CGImage, quality: CGFloat) throws -> Data {
        let buffer = NSMutableData()
        let type = UTType.jpeg.identifier as CFString
        guard let destination = CGImageDestinationCreateWithData(
            buffer as CFMutableData,
            type,
            1,
            nil
        ) else {
            throw Error.encodingFailed
        }
        let properties: [CFString: Any] = [
            kCGImageDestinationLossyCompressionQuality: quality
        ]
        CGImageDestinationAddImage(destination, cgImage, properties as CFDictionary)
        guard CGImageDestinationFinalize(destination) else {
            throw Error.encodingFailed
        }
        return buffer as Data
    }
}

private extension Array where Element == CGRect {
    func union() -> CGRect? {
        guard var result = first else { return nil }
        for rect in dropFirst() {
            result = result.union(rect)
        }
        return result
    }
}

private extension UIImage {
    /// Resize to fit `maxEdge` on the long side, preserving aspect ratio.
    /// Bakes the orientation into the output pixels so downstream code that
    /// reads `cgImage.width/height` sees the correct dimensions.
    func resizedRespectingOrientation(maxEdge: CGFloat) -> UIImage {
        let pixelSize = CGSize(
            width: size.width * scale,
            height: size.height * scale
        )
        let longest = max(pixelSize.width, pixelSize.height)
        let scaleFactor = longest > maxEdge ? maxEdge / longest : 1.0
        let target = CGSize(
            width: pixelSize.width * scaleFactor,
            height: pixelSize.height * scaleFactor
        )

        let format = UIGraphicsImageRendererFormat.default()
        format.scale = 1.0 // we're already in pixel space
        format.opaque = true
        let renderer = UIGraphicsImageRenderer(size: target, format: format)
        return renderer.image { _ in
            draw(in: CGRect(origin: .zero, size: target))
        }
    }
}

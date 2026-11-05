import Foundation
import Observation
import UIKit

/// A verified photo kept on this iPhone.
struct SavedPhoto: Codable, Identifiable, Hashable, Sendable {
    let id: String
    let capturedAt: String
    let cell: String
}

/// The verified photos on this iPhone, newest first. Each file is the signed
/// PNG the laptop returned, so sharing it shares its credentials too.
@MainActor
@Observable
final class Gallery {
    private(set) var photos: [SavedPhoto] = []
    @ObservationIgnored private let folder: URL

    init(folder: URL = URL.applicationSupportDirectory.appending(path: "Verified")) {
        self.folder = folder
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        if let data = try? Data(contentsOf: index), let saved = try? JSONDecoder().decode([SavedPhoto].self, from: data) {
            photos = saved.filter { FileManager.default.fileExists(atPath: file($0).path(percentEncoded: false)) }
        }
    }

    func file(_ photo: SavedPhoto) -> URL { folder.appending(path: "\(photo.id).png") }

    func image(_ photo: SavedPhoto) -> UIImage? { UIImage(contentsOfFile: file(photo).path(percentEncoded: false)) }

    /// Keeps a signed PNG and returns where it lives.
    @discardableResult
    func add(_ photo: SavedPhoto, png: Data) throws -> URL {
        let url = file(photo)
        try png.write(to: url, options: .atomic)
        photos.removeAll { $0.id == photo.id }
        photos.insert(photo, at: 0)
        save()
        return url
    }

    func remove(_ photo: SavedPhoto) {
        try? FileManager.default.removeItem(at: file(photo))
        photos.removeAll { $0.id == photo.id }
        save()
    }

    private var index: URL { folder.appending(path: "index.json") }

    private func save() {
        if let data = try? JSONEncoder().encode(photos) { try? data.write(to: index, options: .atomic) }
    }
}

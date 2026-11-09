// Files and folders from outside the app: picking them, keeping access to them across launches,
// and making sure iCloud has them on this device before Stahl reads them.

import UIKit
import UniformTypeIdentifiers

/// Security-scoped access to what the user picked. iOS grants it per launch; a bookmark per pick
/// brings it back on the next, so the portfolio Stahl last opened opens again by itself.
enum Access {
    private static let key = "picked-bookmarks"
    /// Picks remembered: the last workspace, file, new workspace's location and so on.
    private static let limit = 8
    private static var held: [URL] = []

    /// Resume access to everything picked before. Call before Stahl starts, which reopens the
    /// last portfolio from its own config.
    static func restore() {
        var fresh: [Data] = []
        for bookmark in UserDefaults.standard.array(forKey: key) as? [Data] ?? [] {
            var stale = false
            guard
                let url = try? URL(
                    resolvingBookmarkData: bookmark, options: [], relativeTo: nil,
                    bookmarkDataIsStale: &stale)
            else { continue }
            hold(url)
            fresh.append(stale ? (try? url.bookmarkData()) ?? bookmark : bookmark)
        }
        UserDefaults.standard.set(fresh, forKey: key)
    }

    /// Access to a URL a document picker returned, now and on later launches.
    static func remember(_ url: URL) {
        hold(url)
        guard let bookmark = try? url.bookmarkData() else { return }
        var all = UserDefaults.standard.array(forKey: key) as? [Data] ?? []
        all.removeAll { $0 == bookmark }
        all.insert(bookmark, at: 0)
        UserDefaults.standard.set(Array(all.prefix(limit)), forKey: key)
    }

    private static func hold(_ url: URL) {
        if !held.contains(url), url.startAccessingSecurityScopedResource() {
            held.append(url)
        }
    }
}

/// Presents UIKit's document picker for the app, and hands back a path it can read.
final class Picker: NSObject, UIDocumentPickerDelegate {
    private var done: ((String?) -> Void)?

    func present(folder: Bool, from controller: UIViewController, done: @escaping (String?) -> Void) {
        self.done = done
        let types: [UTType] =
            folder ? [.folder] : [.json, UTType(filenameExtension: "gpg") ?? .data]
        let picker = UIDocumentPickerViewController(forOpeningContentTypes: types, asCopy: false)
        picker.allowsMultipleSelection = false
        picker.delegate = self
        controller.present(picker, animated: true)
    }

    func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
        guard let url = urls.first else { return finish(nil) }
        Access.remember(url)
        Downloads.ensure(url) { [weak self] in self?.finish(url.path) }
    }

    func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
        finish(nil)
    }

    private func finish(_ path: String?) {
        let done = self.done
        self.done = nil
        done?(path)
    }
}

/// iCloud Drive keeps some files only in the cloud until they're opened. Stahl reads plain files,
/// so ask for everything under a pick to be downloaded first, and wait a little for it.
enum Downloads {
    private static let patience: TimeInterval = 20

    static func ensure(_ url: URL, then: @escaping () -> Void) {
        DispatchQueue.global(qos: .userInitiated).async {
            let items = files(under: url)
            for item in items where !downloaded(item) {
                try? FileManager.default.startDownloadingUbiquitousItem(at: item)
            }
            let deadline = Date().addingTimeInterval(patience)
            while Date() < deadline, items.contains(where: { !downloaded($0) }) {
                Thread.sleep(forTimeInterval: 0.25)
            }
            DispatchQueue.main.async(execute: then)
        }
    }

    private static func files(under url: URL) -> [URL] {
        let keys: [URLResourceKey] = [.isDirectoryKey, .ubiquitousItemDownloadingStatusKey]
        guard (try? url.resourceValues(forKeys: [.isDirectoryKey]))?.isDirectory == true else {
            return [url]
        }
        let walk = FileManager.default.enumerator(at: url, includingPropertiesForKeys: keys)
        return (walk?.allObjects as? [URL] ?? []).filter {
            (try? $0.resourceValues(forKeys: [.isDirectoryKey]))?.isDirectory != true
        }
    }

    private static func downloaded(_ url: URL) -> Bool {
        let values = try? url.resourceValues(forKeys: [
            .isUbiquitousItemKey, .ubiquitousItemDownloadingStatusKey,
        ])
        guard values?.isUbiquitousItem == true else { return true }
        return values?.ubiquitousItemDownloadingStatus == .current
    }
}

import MapLibre
import Observation

/// Map areas downloaded for use without a network (MapLibre offline packs).
@MainActor @Observable
final class OfflineMaps {
    /// Areas stored, and their size.
    var areas = 0
    var bytes: UInt64 = 0
    /// Tiles done and expected for the download in progress.
    var progress: (done: UInt64, total: UInt64)?
    var failed = false

    /// Deepest zoom downloaded: footpaths and buildings are visible at 16.
    static let maxZoom = 16.0
    /// Zoom levels below the current one included in a download.
    static let below = 2.0

    @ObservationIgnored private var observers: [NSObjectProtocol] = []

    init() {
        let center = NotificationCenter.default
        observers.append(center.addObserver(forName: .MLNOfflinePackProgressChanged, object: nil, queue: .main) { [weak self] n in
            guard let pack = n.object as? MLNOfflinePack else { return }
            let p = pack.progress
            let complete = pack.state == .complete
            MainActor.assumeIsolated {
                guard let self else { return }
                self.progress = complete ? nil : (p.countOfResourcesCompleted, p.countOfResourcesExpected)
                if complete { self.reload() }
            }
        })
        observers.append(center.addObserver(forName: .MLNOfflinePackError, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.failed = true
                self?.progress = nil
            }
        })
        // The list of packs loads asynchronously at launch.
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { [weak self] in
            MainActor.assumeIsolated { self?.reload() }
        }
    }

    func reload() {
        let packs = MLNOfflineStorage.shared.packs ?? []
        areas = packs.count
        bytes = packs.reduce(0) { $0 + $1.progress.countOfBytesCompleted }
    }

    /// Downloads the visible area, from a little above the current zoom down to street level.
    func download(_ bounds: MLNCoordinateBounds, zoom: Double, style: MapStyle) {
        let from = max(0, floor(zoom) - Self.below)
        let to = min(Self.maxZoom, max(from, floor(zoom) + 4))
        let region = MLNTilePyramidOfflineRegion(styleURL: style.url, bounds: bounds, fromZoomLevel: from, toZoomLevel: to)
        failed = false
        progress = (0, 1)
        MLNOfflineStorage.shared.addPack(for: region, withContext: Data(style.rawValue.utf8)) { [weak self] pack, error in
            MainActor.assumeIsolated {
                if error != nil {
                    self?.failed = true
                    self?.progress = nil
                } else {
                    pack?.resume()
                }
            }
        }
    }

    func deleteAll() {
        for pack in MLNOfflineStorage.shared.packs ?? [] {
            MLNOfflineStorage.shared.removePack(pack) { [weak self] _ in
                MainActor.assumeIsolated { self?.reload() }
            }
        }
    }
}

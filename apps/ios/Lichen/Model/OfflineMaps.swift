import Foundation
import MapLibre
import Observation

/// A map area kept for use without a network: what it covers, and whether it is complete.
struct OfflineArea: Identifiable {
    enum State: Equatable {
        case complete
        case downloading
        /// Stopped before the end (paused, or the app quit): some tiles are missing.
        case partial
        case failed(String)
    }

    let id: ObjectIdentifier
    let pack: MLNOfflinePack
    let name: String
    let style: String
    let created: Date?
    let done: UInt64
    let total: UInt64
    let bytes: UInt64
    let state: State
}

/// Map areas downloaded for use without a network (MapLibre offline packs), one by one:
/// complete, partial, failed or downloading, never just a count (audit M11).
@MainActor @Observable
final class OfflineMaps {
    var areas: [OfflineArea] = []
    /// An area could not even be created (no network, no storage).
    var lastError: String?

    /// Deepest zoom downloaded: footpaths and buildings are visible at 16.
    static let maxZoom = 16.0
    /// Zoom levels below the current one included in a download.
    static let below = 2.0

    @ObservationIgnored private var observers: [NSObjectProtocol] = []
    @ObservationIgnored private var errors: [ObjectIdentifier: String] = [:]

    /// The download in progress, for the map's banner.
    var progress: (done: UInt64, total: UInt64)? {
        areas.first { $0.state == .downloading }.map { ($0.done, max($0.total, 1)) }
    }

    var bytes: UInt64 { areas.reduce(0) { $0 + $1.bytes } }

    init() {
        let center = NotificationCenter.default
        observers.append(center.addObserver(forName: .MLNOfflinePackProgressChanged, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.reload() }
        })
        observers.append(center.addObserver(forName: .MLNOfflinePackError, object: nil, queue: .main) { [weak self] n in
            let pack = n.object as? MLNOfflinePack
            let error = (n.userInfo?[MLNOfflinePackUserInfoKey.error] as? Error)?.localizedDescription
            MainActor.assumeIsolated {
                guard let self, let pack else { return }
                self.errors[ObjectIdentifier(pack)] = error ?? String(localized: "The download failed.")
                pack.suspend()
                self.reload()
            }
        })
        observers.append(center.addObserver(forName: .MLNOfflinePackMaximumMapboxTilesReached, object: nil, queue: .main) { [weak self] n in
            let pack = n.object as? MLNOfflinePack
            MainActor.assumeIsolated {
                guard let self, let pack else { return }
                self.errors[ObjectIdentifier(pack)] = String(localized: "Too large: zoom in and download a smaller area.")
                pack.suspend()
                self.reload()
            }
        })
        // The list of packs loads asynchronously at launch; then their real progress.
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { [weak self] in
            MainActor.assumeIsolated {
                for p in MLNOfflineStorage.shared.packs ?? [] { p.requestProgress() }
                self?.reload()
            }
        }
    }

    func reload() {
        areas = (MLNOfflineStorage.shared.packs ?? []).map { pack in
            let meta = (try? JSONSerialization.jsonObject(with: pack.context)) as? [String: Any]
            let id = ObjectIdentifier(pack)
            let p = pack.progress
            let state: OfflineArea.State
            if let e = errors[id] {
                state = .failed(e)
            } else {
                switch pack.state {
                case .complete: state = .complete
                case .active: state = .downloading
                default: state = .partial
                }
            }
            return OfflineArea(
                id: id, pack: pack,
                name: meta?["name"] as? String ?? String(localized: "Map area"),
                // Packs from before carry just the style's name.
                style: meta?["style"] as? String ?? String(data: pack.context, encoding: .utf8) ?? "",
                created: (meta?["created"] as? Double).map { Date(timeIntervalSince1970: $0) },
                done: p.countOfResourcesCompleted, total: p.countOfResourcesExpected,
                bytes: p.countOfBytesCompleted, state: state
            )
        }
        .sorted { ($0.created ?? .distantPast) > ($1.created ?? .distantPast) }
    }

    /// Downloads the visible area, from a little above the current zoom down to street level.
    func download(_ bounds: MLNCoordinateBounds, zoom: Double, style: MapStyle) {
        let from = max(0, floor(zoom) - Self.below)
        let to = min(Self.maxZoom, max(from, floor(zoom) + 4))
        let region = MLNTilePyramidOfflineRegion(styleURL: style.url, bounds: bounds, fromZoomLevel: from, toZoomLevel: to)
        let c = CLLocationCoordinate2D(latitude: (bounds.sw.latitude + bounds.ne.latitude) / 2,
                                       longitude: (bounds.sw.longitude + bounds.ne.longitude) / 2)
        let name = String(format: "%.3f, %.3f", c.latitude, c.longitude)
            + " · " + String(localized: "zoom \(Int(from))–\(Int(to))")
        let meta: [String: Any] = ["name": name, "style": style.rawValue, "created": Date().timeIntervalSince1970]
        let context = (try? JSONSerialization.data(withJSONObject: meta)) ?? Data()
        lastError = nil
        MLNOfflineStorage.shared.addPack(for: region, withContext: context) { [weak self] pack, error in
            MainActor.assumeIsolated {
                if let error {
                    self?.lastError = error.localizedDescription
                } else {
                    pack?.resume()
                }
                self?.reload()
            }
        }
    }

    /// Resumes a partial or failed area.
    func resume(_ area: OfflineArea) {
        errors[area.id] = nil
        area.pack.resume()
        reload()
    }

    func pause(_ area: OfflineArea) {
        area.pack.suspend()
        reload()
    }

    func delete(_ area: OfflineArea) {
        errors[area.id] = nil
        MLNOfflineStorage.shared.removePack(area.pack) { [weak self] _ in
            MainActor.assumeIsolated { self?.reload() }
        }
    }
}

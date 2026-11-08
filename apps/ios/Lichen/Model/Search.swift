import CoreLocation
import Foundation
import UIKit

/// Who an alert or a search is about (a node key), for `fullScreenCover(item:)`.
struct SenderID: Identifiable, Hashable {
    let id: Data
}

/// An SOS heard from one person (protocol.md §11-12): its positions or, without GPS, its
/// beacons, until it ends. One alert per incident.
struct SosIncident: Equatable {
    let from: Data
    /// The group it came through (`nil`: a direct share).
    var group: Data?
    /// Its last position; `nil` for an SOS without GPS (beacons only).
    var position: PeerPosition?
    var battery: UInt8?
    let started: Date
    var lastHeard: Date
    var ended: Bool

    enum State { case active, stale, ended }

    /// Stale after five minutes without anything (beacons come every two).
    func state(_ now: Date = .now) -> State {
        if ended { return .ended }
        return now.timeIntervalSince(lastHeard) > 5 * 60 ? .stale : .active
    }
}

extension AppModel {
    // MARK: SOS incidents

    /// An SOS position or beacon from `from`: a new incident (alert, critical notification) or
    /// the current one, kept up to date.
    func heardSos(from: Data, group: Data?, position: PeerPosition?, battery: UInt8?) {
        let now = Date()
        if var i = incidents[from], !i.ended {
            i.lastHeard = now
            if let position { i.position = position }
            if let group { i.group = group }
            if let battery { i.battery = battery }
            incidents[from] = i
            return
        }
        incidents[from] = SosIncident(from: from, group: group, position: position, battery: battery,
                                      started: now, lastHeard: now, ended: false)
        if searchTarget.flatMap({ incidents[$0] })?.state() != .active { searchTarget = from }
        #if DEBUG
        // -screen search: straight to the search, for screenshots.
        if debugScreen == "search" {
            _ = startSearch(from)
            tab = .search
            return
        }
        #endif
        sosAlert = SenderID(id: from)
        notifier.sos(.contact(key: from),
                     title: String(localized: "SOS from \(name(of: from))"),
                     body: position == nil
                        ? String(localized: "No GPS position: search for them by radio from lichen.")
                        : String(localized: "Their exact position is on the map; open lichen to follow it."))
    }

    /// `from` left SOS (an end position, a normal one, or a beacon without the SOS bit).
    func endSos(_ from: Data) {
        guard var i = incidents[from], !i.ended else { return }
        i.ended = true
        incidents[from] = i
        if sosAlert?.id == from { sosAlert = nil }
    }

    /// Incidents still going on, newest first.
    var activeIncidents: [SosIncident] {
        incidents.values.filter { $0.state() != .ended }.sorted { $0.started > $1.started }
    }

    // MARK: searching (protocol.md §11)

    /// The people the Search tab can show: incidents not ended, and every search we run.
    var searchTargets: [Data] {
        var out = activeIncidents.map(\.from)
        for s in searches where !out.contains(s.node) { out.append(s.node) }
        if let t = searchTarget, incidents[t] != nil, !out.contains(t) { out.append(t) }
        return out
    }

    func search(for node: Data) -> SearchState? {
        searches.first { $0.node == node }
    }

    /// The group to search `node` in: the incident's, else the first group with them.
    func searchGroup(for node: Data) -> Data? {
        if let g = incidents[node]?.group, groups.contains(where: { $0.gid == g }) { return g }
        return groups.first { $0.members.contains { $0.key == node } }?.gid
    }

    /// Starts a search for `node`; an error to show, or nil.
    func startSearch(_ node: Data) -> String? {
        guard let gid = searchGroup(for: node) else {
            return String(localized: "A radio search needs a group you share with them.")
        }
        do {
            try core.startSearch(gid: gid, target: node, nowMs: now())
        } catch let e as LichenError {
            return e.display
        } catch {
            return nil
        }
        searchTarget = node
        refreshSearches()
        pump()
        return nil
    }

    /// "Let this group locate me" (kept across restarts by lc-ffi).
    func searchConsent(_ gid: Data) -> Bool {
        core.searchConsent(gid: gid)
    }

    func setSearchConsent(_ gid: Data, _ on: Bool) {
        try? core.setSearchConsent(gid: gid, on: on, nowMs: now())
        pump()
    }

    func stopSearch(_ node: Data) {
        try? core.stopSearch(target: node)
        refreshSearches()
    }

    func ping(_ node: Data) -> String? {
        searchCall { try core.searchPing(target: node, nowMs: now()) }
    }

    func addBearing(_ node: Data, degrees: Double, yagi: Bool) -> String? {
        searchCall { try core.searchBearing(target: node, deg: degrees, yagi: yagi, nowMs: now()) }
    }

    func lowerPower(_ node: Data, dbm: Int8) -> String? {
        searchCall { try core.searchLowerPower(target: node, dbm: dbm, minutes: 10, nowMs: now()) }
    }

    private func searchCall(_ f: () throws -> Void) -> String? {
        do {
            try f()
        } catch let e as LichenError {
            return e.display
        } catch {
            return nil
        }
        refreshSearches()
        pump()
        return nil
    }

    func searchEstimate(_ node: Data) -> SearchEstimate? {
        core.searchEstimate(target: node)
    }

    func measurements(_ node: Data) -> [SearchMeasurement] {
        core.searchMeasurements(target: node)
    }

    /// The search's measurements as a CSV file to share (field tests).
    func searchCSV(_ node: Data) -> URL? {
        let csv = core.searchCsv(target: node)
        let safe = name(of: node).filter { $0.isLetter || $0.isNumber }
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("lichen-search-\(safe.isEmpty ? "target" : safe).csv")
        guard (try? csv.write(to: url, atomically: true, encoding: .utf8)) != nil else { return nil }
        return url
    }

    func refreshSearches() {
        let t = now()
        let s = core.searches(nowMs: t)
        if s != searches {
            searches = s
            updateLocator()
        }
        searchedBy = core.searchedBy(nowMs: t)
    }

    /// The phone's battery, for our beacons (at most once a minute).
    func reportBattery() {
        let t = Date()
        if let last = lastBatteryReport, t.timeIntervalSince(last) < 60 { return }
        lastBatteryReport = t
        UIDevice.current.isBatteryMonitoringEnabled = true
        let level = UIDevice.current.batteryLevel
        core.setBattery(percent: level < 0 ? nil : UInt8((level * 100).rounded()))
    }

    // MARK: who needs our position, and the compass

    /// `who` (map, search, alert) needs our position while on screen.
    func needsLocation(_ who: String, _ on: Bool) {
        if on { locationUsers.insert(who) } else { locationUsers.remove(who) }
    }

    /// The compass, while the Search tab is on screen.
    func useHeading(_ on: Bool) {
        if on {
            locator.onHeading = { [weak self] h in self?.heading = h }
            locator.startHeading()
        } else {
            locator.stopHeading()
            heading = nil
        }
    }
}

extension SearchEstimate {
    /// The heat grid as an image (SOS red, transparent where nothing is likely).
    func heatImage() -> UIImage? {
        let (w, h) = (Int(heatCols), Int(heatRows))
        guard w > 0, h > 0, heat.count == w * h else { return nil }
        var rgba = [UInt8](repeating: 0, count: w * h * 4)
        for (i, v) in heat.enumerated() {
            let a = Double(v) / 255
            // Premultiplied red.
            let alpha = UInt8((a * 0.85 * 255).rounded())
            rgba[i * 4] = UInt8((Double(alpha) * 0.86).rounded())
            rgba[i * 4 + 1] = UInt8((Double(alpha) * 0.15).rounded())
            rgba[i * 4 + 2] = UInt8((Double(alpha) * 0.15).rounded())
            rgba[i * 4 + 3] = alpha
        }
        let space = CGColorSpaceCreateDeviceRGB()
        guard let provider = CGDataProvider(data: Data(rgba) as CFData),
              let image = CGImage(width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: w * 4,
                                  space: space, bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
                                  provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent)
        else { return nil }
        return UIImage(cgImage: image)
    }

    /// Whether a point lies inside the 68 % ellipse.
    func contains68(lat p: Double, lon q: Double) -> Bool {
        let th = azimuthDeg * .pi / 180
        let east = (q - lon) * 111_320 * max(0.01, cos(lat * .pi / 180))
        let north = (p - lat) * 111_320
        let u = east * sin(th) + north * cos(th)
        let v = east * cos(th) - north * sin(th)
        let a = max(major68M, 1), b = max(minor68M, 1)
        return (u / a) * (u / a) + (v / b) * (v / b) <= 1
    }

    /// The ellipse with semi-axes `major`, `minor` (metres) around the estimate.
    func ellipse(major: Double, minor: Double) -> [CLLocationCoordinate2D] {
        let th = azimuthDeg * .pi / 180
        let mPerLat = 111_320.0, mPerLon = 111_320.0 * max(0.01, cos(lat * .pi / 180))
        return (0...64).map { i in
            let t = Double(i) / 64 * 2 * .pi
            let (x, y) = (major * cos(t), minor * sin(t))
            let east = x * sin(th) + y * cos(th)
            let north = x * cos(th) - y * sin(th)
            return CLLocationCoordinate2D(latitude: lat + north / mPerLat, longitude: lon + east / mPerLon)
        }
    }
}


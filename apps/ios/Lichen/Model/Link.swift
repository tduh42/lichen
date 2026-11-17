import SwiftUI
import UIKit

/// How the radio is reached.
enum RadioLink: Equatable {
    case bluetooth
    /// TCP: a companion on Wi-Fi, or a USB radio relayed by `lichen bridge` (`TcpCompanion`).
    case tcp
}

extension Companion.State {
    /// The state in a word for the problem report's log, without the radio's name.
    var logWord: String {
        switch self {
        case .off: "off"
        case .idle: "idle"
        case .scanning: "scanning"
        case .connecting: "connecting"
        case .connected: "connected"
        }
    }
}

/// The radio links (Bluetooth or TCP) and the app's log for problem reports (ADR-0045). A TCP
/// link the user asked for comes first: Bluetooth's state and frames are left aside while it
/// is wanted, as on Android.
extension AppModel {
    /// Wires the TCP companion; the link in use when the app last stopped comes back.
    func setUpTcp() {
        tcp.onState = { [weak self] s in MainActor.assumeIsolated { self?.tcpChanged(s) } }
        tcp.onFrame = { [weak self] d in MainActor.assumeIsolated { self?.frame(d) } }
        tcp.onFailure = { [weak self] why in MainActor.assumeIsolated { self?.tcpError = why } }
        tcp.onLog = { [weak self] line in MainActor.assumeIsolated { self?.note(line) } }
        tcp.reconnectLast()
        #if DEBUG
        // -tcp 127.0.0.1:5000: the simulator on a real radio through `lichen bridge`.
        if let address = UserDefaults.standard.string(forKey: "tcp") { connectTcp(address) }
        #endif
    }

    /// The state of the link in use.
    var linkState: Companion.State {
        tcp.target != nil ? tcp.state : companion.state
    }

    /// Connects to the companion at `text` ("host:port"); false when it is not an address.
    @discardableResult
    func connectTcp(_ text: String) -> Bool {
        tcpError = nil
        return tcp.connect(text)
    }

    /// Disconnects the radio in use (and forgets it).
    func disconnectRadio() {
        if tcp.target != nil {
            tcp.disconnect()
        } else {
            companion.disconnect()
        }
    }

    /// The banner's Retry: the TCP link tries at once, Bluetooth scans.
    func retryRadio() {
        if tcp.target != nil {
            tcp.retryNow()
        } else {
            companion.scan()
        }
    }

    /// Writes a frame to the radio in use.
    func writeRadio(_ frame: Data) {
        if tcp.target != nil {
            tcp.write(frame)
        } else {
            companion.write(frame)
        }
    }

    /// A frame from Bluetooth: dropped while the TCP link is wanted (both may reach a radio).
    func bluetoothFrame(_ data: Data) {
        if tcp.target == nil { frame(data) }
    }

    /// The app came to the foreground or left it.
    func scene(_ phase: ScenePhase) {
        switch phase {
        case .active:
            note("app in the foreground")
            tcp.retryNow()
        case .background:
            note("app in the background")
        default:
            break
        }
    }

    private func tcpChanged(_ s: Companion.State) {
        note("TCP: \(s.logWord)")
        if tcp.target != nil {
            companion.stopScan()
            link = .tcp
            linkChanged(s)
        } else {
            link = .bluetooth
            linkChanged(companion.state)
        }
    }

    // MARK: problem report (ADR-0045)

    /// Adds a line to the app's log: fixed words and numbers, never a name, text, key or
    /// position (the core masks them anyway).
    func note(_ text: String) {
        appLog.append(ReportLine(atMs: now(), text: text))
        if appLog.count > 200 { appLog.removeFirst(appLog.count - 200) }
    }

    /// The problem report: versions, the radio, settings, and the core's and the app's recent
    /// log, without message content.
    func problemReport() -> String {
        let info = Bundle.main.infoDictionary
        let version = "\(info?["CFBundleShortVersionString"] as? String ?? "?") (\(info?["CFBundleVersion"] as? String ?? "?"))"
        let link = isDemo ? "demo" : (self.link == .tcp ? "TCP" : "Bluetooth")
        let settings = [
            "iroh: \(irohEnabled ? (relayOnly ? "on, relays only" : "on") : "off")",
            "Tor: \(torEnabled ? "on" : "off") (\(torStatus))",
            "GIFs: \(gifAutoLoad ? "load automatically" : "tap to load")",
            "voice mode: \(voiceMode)",
            "radio details: \(UserDefaults.standard.bool(forKey: radioDetailsKey) ? "shown" : "behind a tap")",
            "app lock: \(lock.enabled ? "on" : "off")",
            "language: \(Locale.preferredLanguages.first ?? "?")",
        ]
        var privateWords = [myName, nodeName] + found.map(\.name)
        if let host = tcp.target?.host { privateWords.append(host) }
        let app = ReportApp(
            appVersion: version,
            platform: "\(UIDevice.current.systemName) \(UIDevice.current.systemVersion)",
            device: Self.deviceModel(),
            link: link,
            settings: settings,
            log: appLog,
            privateWords: privateWords
        )
        return core.problemReport(app: app, nowMs: now())
    }

    /// "iPhone17,3"; the simulated model on the simulator.
    private static func deviceModel() -> String {
        if let sim = ProcessInfo.processInfo.environment["SIMULATOR_MODEL_IDENTIFIER"] { return "\(sim) (simulator)" }
        var u = utsname()
        uname(&u)
        return withUnsafeBytes(of: &u.machine) { raw in
            String(decoding: raw.prefix(while: { $0 != 0 }), as: UTF8.self)
        }
    }
}

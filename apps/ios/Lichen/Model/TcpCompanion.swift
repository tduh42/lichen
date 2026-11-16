import Foundation
import Network

/// The MeshCore companion over TCP, with the serial interface's framing (`'<' len frame` out,
/// `'>' len frame` in), as on Android (`TcpCompanion.kt`): a companion on Wi-Fi, or a USB radio
/// relayed by `lichen bridge` on a computer (the simulator reaches its Mac at 127.0.0.1).
/// The link is neither encrypted nor authenticated (review of 2026-11-15, SR-31): the pairing
/// screen says so.
///
/// A link that was up and drops is tried again after 1, 2, 4, 8, 16 s, then every 30 s
/// (`linkRetryDelayMs`), until the user disconnects; a first attempt that fails is reported
/// instead. The address in use is remembered, and tried again at the next launch.
final class TcpCompanion {
    /// What the app is told; `.connecting` while a dropped link waits for its next attempt.
    var onState: ((Companion.State) -> Void)?
    var onFrame: ((Data) -> Void)?
    /// A first attempt failed: why, in the system's words.
    var onFailure: ((String) -> Void)?
    /// A line for the problem report's log (no address in it).
    var onLog: ((String) -> Void)?

    private(set) var state: Companion.State = .idle {
        didSet { if state != oldValue { onState?(state) } }
    }

    /// The address while the link is wanted: connected, connecting, or waiting to retry.
    private(set) var target: TcpAddress?
    private var connection: NWConnection?
    private var decoder: UsbDecoder?
    private var attempt: UInt32 = 0
    private var retry: DispatchWorkItem?
    /// The link was up once since the user asked for it: only then is a failure retried.
    private var wasUp = false
    private static let lastAddress = "lichen.tcpAddress"
    private static let active = "lichen.tcpActive"

    /// The address last typed, to offer again; the simulator reaches its Mac at 127.0.0.1.
    static var lastTyped: String {
        UserDefaults.standard.string(forKey: lastAddress) ?? "127.0.0.1:5000"
    }

    /// "host:port" of the link wanted.
    var label: String? {
        target.map { $0.host.contains(":") ? "[\($0.host)]:\($0.port)" : "\($0.host):\($0.port)" }
    }

    /// Connects to what the user typed and keeps the link until `disconnect`; false when it is
    /// not an address.
    @discardableResult
    func connect(_ text: String) -> Bool {
        guard let address = tcpAddress(text: text) else { return false }
        UserDefaults.standard.set(text.trimmingCharacters(in: .whitespaces), forKey: Self.lastAddress)
        stop()
        target = address
        wasUp = false
        attempt = 0
        open()
        return true
    }

    /// The link in use when the app last stopped, tried again (and retried) at launch.
    func reconnectLast() {
        guard UserDefaults.standard.bool(forKey: Self.active), connection == nil,
              let address = tcpAddress(text: Self.lastTyped) else { return }
        target = address
        wasUp = true
        attempt = 0
        open()
    }

    /// The app came back to the foreground: a link waiting to retry tries at once (iOS may have
    /// cut it while the app was in the background).
    func retryNow() {
        guard retry != nil else { return }
        retry?.cancel()
        retry = nil
        open()
    }

    /// Closes the link and forgets it.
    func disconnect() {
        UserDefaults.standard.set(false, forKey: Self.active)
        stop()
        target = nil
        state = .idle
        onLog?("TCP: disconnected by the user")
    }

    /// Writes one companion frame.
    func write(_ frame: Data) {
        guard let connection, case .connected = state,
              let wire = try? usbEncode(frame: frame) else { return }
        connection.send(content: wire, completion: .contentProcessed { _ in })
    }

    private func stop() {
        retry?.cancel()
        retry = nil
        connection?.cancel()
        connection = nil
        decoder = nil
    }

    private func open() {
        guard let target, let port = NWEndpoint.Port(rawValue: target.port) else { return }
        let tcp = NWProtocolTCP.Options()
        tcp.noDelay = true
        tcp.connectionTimeout = 5
        // A radio switched off on Wi-Fi sends nothing: keepalives find the dead link in ~25 s.
        tcp.enableKeepalive = true
        tcp.keepaliveIdle = 10
        tcp.keepaliveInterval = 5
        tcp.keepaliveCount = 3
        let c = NWConnection(host: NWEndpoint.Host(target.host), port: port, using: NWParameters(tls: nil, tcp: tcp))
        connection = c
        decoder = UsbDecoder()
        state = .connecting(label ?? "")
        c.stateUpdateHandler = { [weak self] st in
            guard let self, c === self.connection else { return }
            switch st {
            case .ready:
                attempt = 0
                wasUp = true
                UserDefaults.standard.set(true, forKey: Self.active)
                state = .connected(label ?? "")
                onLog?("TCP: connected")
                receive(c)
            case let .waiting(error), let .failed(error):
                dropped(c, error.localizedDescription)
            default:
                break
            }
        }
        c.start(queue: .main)
    }

    private func receive(_ c: NWConnection) {
        c.receive(minimumIncompleteLength: 1, maximumLength: 4096) { [weak self] data, _, complete, error in
            guard let self, c === self.connection else { return }
            if let data, !data.isEmpty, let decoder {
                for f in decoder.push(bytes: data) { onFrame?(f) }
            }
            if complete || error != nil {
                dropped(c, error?.localizedDescription ?? String(localized: "The radio closed the link."))
            } else {
                receive(c)
            }
        }
    }

    /// The connection failed or was lost: retried when the link was up once, else reported.
    private func dropped(_ c: NWConnection, _ why: String) {
        c.cancel()
        connection = nil
        decoder = nil
        guard target != nil else { return }
        if wasUp {
            attempt += 1
            let delay = linkRetryDelayMs(attempt: attempt)
            onLog?("TCP: link lost, attempt \(attempt) in \(delay / 1000) s")
            state = .connecting(label ?? "")
            let work = DispatchWorkItem { [weak self] in
                guard let self, self.retry != nil else { return }
                self.retry = nil
                self.open()
            }
            retry = work
            DispatchQueue.main.asyncAfter(deadline: .now() + .milliseconds(Int(delay)), execute: work)
        } else {
            target = nil
            state = .idle
            onLog?("TCP: could not connect")
            onFailure?(why)
        }
    }
}

import Foundation
import Network

/// A radio on the network, as `lichen bridge` serves one: the demo network's companion behind a
/// TCP port on this Mac, which the simulator shares, with the serial framing ('<' len frame
/// from the app, '>' len frame to it). The app reaches it through its real TCP link
/// (`-tcp 127.0.0.1:port`), as a radio it pairs with and saves: not the demo mode, so what the
/// app stores is kept across launches (LC-4). The simulation runs on the wall clock, its script
/// 20 s sooner, from the app's first connection: however long the simulator takes to start the
/// app, the script meets it at the same time (B-ios-31: started with the test, a fresh install
/// that took 11.5 s or more to connect missed Camille's first handshake, pushed to nobody, and
/// her retry fell on Marc's only message at the hub). Otherwise as Android's FakeTcpRadio.
final class FakeTcpRadio {
    private let demo = DemoNetwork()
    /// Says nothing of its own (B-user-6): its answers to the app, never a push (a message
    /// heard, an advert), as a radio on a quiet mesh, so nothing else wakes the app.
    private let quiet: Bool
    private let listener: NWListener
    private let queue = DispatchQueue(label: "fake-tcp-radio")
    /// When the app first connected: the simulation's time 0. Nothing runs before.
    private var started: DispatchTime?
    private var connection: NWConnection?
    private var input = Data()
    private var timer: DispatchSourceTimer?
    private(set) var port: UInt16 = 0

    /// "127.0.0.1:port", as the app's `-tcp` takes it.
    var address: String { "127.0.0.1:\(port)" }

    init(quiet: Bool = false) throws {
        self.quiet = quiet
        demo.skipAhead(seconds: 20)
        let parameters = NWParameters.tcp
        parameters.requiredInterfaceType = .loopback
        listener = try NWListener(using: parameters, on: .any)
        let ready = DispatchSemaphore(value: 0)
        listener.stateUpdateHandler = { state in
            if case .ready = state { ready.signal() }
        }
        listener.newConnectionHandler = { [weak self] c in self?.accept(c) }
        listener.start(queue: queue)
        _ = ready.wait(timeout: .now() + 10)
        port = listener.port?.rawValue ?? 0
        // What the radio pushes on its own (messages heard, adverts), every 40 ms.
        let t = DispatchSource.makeTimerSource(queue: queue)
        t.schedule(deadline: .now(), repeating: .milliseconds(40))
        t.setEventHandler { [weak self] in self?.flush() }
        t.resume()
        timer = t
    }

    deinit { stop() }

    /// The link closed and the radio gone.
    func stop() {
        timer?.cancel()
        connection?.cancel()
        listener.cancel()
    }

    private func now() -> UInt64 {
        guard let started else { return 0 }
        return (DispatchTime.now().uptimeNanoseconds - started.uptimeNanoseconds) / 1_000_000
    }

    /// One app at a time: a new connection replaces the old one (the app restarted).
    private func accept(_ c: NWConnection) {
        if started == nil { started = .now() }
        connection?.cancel()
        connection = c
        input = Data()
        c.start(queue: queue)
        receive(c)
    }

    private func receive(_ c: NWConnection) {
        c.receive(minimumIncompleteLength: 1, maximumLength: 65_536) { [weak self] data, _, done, error in
            guard let self, c === self.connection else { return }
            if let data { self.input.append(data); self.frames() }
            if done || error != nil { c.cancel(); return }
            self.receive(c)
        }
    }

    /// The app's frames: '<', a little-endian length, the frame.
    private func frames() {
        while true {
            guard let start = input.firstIndex(of: UInt8(ascii: "<")) else { input.removeAll(); return }
            input.removeSubrange(input.startIndex..<start)
            guard input.count >= 3 else { return }
            let bytes = [UInt8](input.prefix(3))
            let length = Int(bytes[1]) | Int(bytes[2]) << 8
            guard input.count >= 3 + length else { return }
            let frame = Data(input.dropFirst(3).prefix(length))
            input.removeSubrange(input.startIndex..<input.index(input.startIndex, offsetBy: 3 + length))
            demo.write(frame: frame, nowMs: now())
            flush()
        }
    }

    /// The radio's frames to the app: '>', a little-endian length, the frame.
    private func flush() {
        guard started != nil else { return }
        // Pushes are 0x80 and up (lc-meshcore `codes::resp`).
        let frames = demo.read(nowMs: now()).filter { !quiet || ($0.first ?? 0) < 0x80 }
        guard let c = connection, !frames.isEmpty else { return }
        var out = Data()
        for f in frames {
            out.append(UInt8(ascii: ">"))
            out.append(UInt8(f.count & 0xFF))
            out.append(UInt8(f.count >> 8))
            out.append(f)
        }
        c.send(content: out, completion: .idempotent)
    }
}

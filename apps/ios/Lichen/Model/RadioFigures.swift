import Foundation

/// The radio's own figures (protocol §16.4, `GET_STATS`) in Radio settings' "Since it started",
/// in their order there: only those the radio reported, never a 0 standing for one it did not.
enum RadioFigure: String, CaseIterable, Identifiable {
    case uptime
    case packetsReceived = "packets_received"
    case packetsSent = "packets_sent"
    case receiveErrors = "receive_errors"
    case airtimeSent = "airtime_sent"
    case airtimeReceived = "airtime_received"
    case lastPacket = "last_packet"

    var id: String { rawValue }

    var title: String {
        switch self {
        case .uptime: String(localized: "On for")
        case .packetsReceived: String(localized: "Packets received")
        case .packetsSent: String(localized: "Packets sent")
        case .receiveErrors: String(localized: "Received with errors")
        case .airtimeSent: String(localized: "Airtime sent")
        case .airtimeReceived: String(localized: "Airtime received")
        case .lastPacket: String(localized: "Last packet heard")
        }
    }

    var icon: String {
        switch self {
        case .uptime: "power"
        case .packetsReceived: "tray.and.arrow.down"
        case .packetsSent: "tray.and.arrow.up"
        case .receiveErrors: "exclamationmark.circle"
        case .airtimeSent: "arrow.up.right"
        case .airtimeReceived: "arrow.down.left"
        case .lastPacket: "ear"
        }
    }
}

extension RadioSettings {
    /// The figures this radio reported, in their order on screen.
    var figures: [RadioFigure] {
        RadioFigure.allCases.filter { f in
            switch f {
            case .uptime: uptimeS != nil
            case .packetsReceived: packetsReceived != nil
            case .packetsSent: packetsSent != nil
            case .receiveErrors: receiveErrors != nil
            case .airtimeSent: txAirtimeS != nil
            case .airtimeReceived: rxAirtimeS != nil
            case .lastPacket: lastRssiDbm != nil || lastSnrDb != nil
            }
        }
    }

    /// What a figure's row says after its title, in the app's language: "3 d 4 h", "1,204",
    /// "12 · 1 % of received", "4 min 12 s · 0.4 % of the time", "−92 dBm · 6.25 dB".
    func text(_ f: RadioFigure) -> String {
        switch f {
        case .uptime:
            return uptimeS.map { $0 < 60 ? String(localized: "\(Int($0)) s") : radioDuration($0) } ?? ""
        case .packetsReceived: return packetsReceived.map(countText) ?? ""
        case .packetsSent: return packetsSent.map(countText) ?? ""
        case .receiveErrors:
            guard let errors = receiveErrors else { return "" }
            guard let share = shareText(errors, of: packetsReceived) else { return countText(errors) }
            return String(localized: "\(countText(errors)) · \(share) % of received")
        case .airtimeSent: return txAirtimeS.map(airtimeShare) ?? ""
        case .airtimeReceived: return rxAirtimeS.map(airtimeShare) ?? ""
        case .lastPacket:
            return [lastRssiDbm.map(dbmText), lastSnrDb.map(snrText)].compactMap { $0 }.joined(separator: " · ")
        }
    }

    private func airtimeShare(_ s: UInt32) -> String {
        let text = airtimeText(s)
        guard let share = shareText(s, of: uptimeS) else { return text }
        return String(localized: "\(text) · \(share) % of the time")
    }
}

/// A power in dBm as the radio gives it: "−118 dBm".
func dbmText(_ dbm: Int16) -> String {
    (dbm < 0 ? "−" : "") + "\(abs(Int(dbm))) dBm"
}

/// An SNR as the radio gives it, up to two decimals: "6.25 dB", "−3.5 dB".
private func snrText(_ db: Float) -> String {
    (db < 0 ? "−" : "") + Double(abs(db)).formatted(.number.precision(.fractionLength(0...2)).grouping(.never)) + " dB"
}

/// "1,204" ("1 204" in French).
private func countText(_ n: UInt32) -> String {
    Int(n).formatted(.number)
}

/// A share in percent: one decimal under 1 % ("0.4"), none above ("12"); none of nothing.
func shareText(_ part: UInt32, of whole: UInt32?) -> String? {
    guard let whole, whole > 0 else { return nil }
    let p = Double(part) / Double(whole) * 100
    return p > 0 && p < 1 ? decimal(p, 1) : decimal(p, 0)
}

/// Airtime: "42 s", "4 min 12 s", then as `radioDuration`.
private func airtimeText(_ s: UInt32) -> String {
    if s < 60 { return String(localized: "\(Int(s)) s") }
    if s < 3_600 { return String(localized: "\(Int(s / 60)) min \(Int(s % 60)) s") }
    return radioDuration(s)
}

/// "3 d 4 h", "2 h 5 min", "42 min" (French "3 j 4 h"), as Android says a time up.
func radioDuration(_ s: UInt32) -> String {
    let s = Int(s)
    if s >= 86_400 { return String(localized: "\(s / 86_400) d \(s % 86_400 / 3_600) h") }
    if s >= 3_600 { return String(localized: "\(s / 3_600) h \(s % 3_600 / 60) min") }
    return String(localized: "\(s / 60) min")
}

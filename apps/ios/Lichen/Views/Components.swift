import SwiftUI

/// The transport, always written (design principle 3).
struct TransportBadge: View {
    enum Kind { case lora, iroh, tor, neutral }

    let kind: Kind
    let label: String

    var body: some View {
        switch kind {
        case .lora: Badge(text: label, background: Theme.loraBg, foreground: Theme.loraFg)
        case .iroh: Badge(text: label, background: Theme.irohBg, foreground: Theme.irohFg)
        case .tor: Badge(text: label, background: Theme.torBg, foreground: Theme.torFg)
        case .neutral: Badge(text: label, background: Theme.s2, foreground: Theme.tx2)
        }
    }
}

/// Airtime left this hour: big figure and a bar.
struct AirtimeMeter: View {
    let usedMs: UInt64
    let limitMs: UInt64
    var large = false
    /// The figure's sizes, following Dynamic Type.
    @ScaledMetric(relativeTo: .largeTitle) private var bigFigure: CGFloat = 40
    @ScaledMetric(relativeTo: .title3) private var smallFigure: CGFloat = 20

    var body: some View {
        VStack(alignment: .leading, spacing: large ? 10 : 8) {
            if large {
                HStack(alignment: .lastTextBaseline) {
                    Text("\(seconds(left)) s")
                        .font(.system(size: bigFigure, weight: .bold).monospacedDigit())
                        .foregroundStyle(Theme.tx)
                    Spacer()
                    Text("of \(seconds(limitMs)) s / hour")
                        .font(.system(.footnote))
                        .foregroundStyle(Theme.tx2)
                }
            } else {
                HStack(alignment: .firstTextBaseline) {
                    Text("Airtime left this hour")
                        .font(.system(.footnote))
                        .foregroundStyle(Theme.tx2)
                    Spacer()
                    Text("\(seconds(left)) s").font(.system(size: smallFigure, weight: .bold).monospacedDigit()).foregroundStyle(Theme.tx)
                        + Text(" / \(seconds(limitMs)) s").font(.system(.footnote)).foregroundStyle(Theme.tx2)
                }
            }
            Bar(fraction: Double(left) / Double(max(limitMs, 1)), color: Theme.lora)
        }
        .accessibilityElement(children: .combine)
    }

    private var left: UInt64 { limitMs - min(usedMs, limitMs) }
}

/// A thin rounded progress bar.
struct Bar: View {
    let fraction: Double
    var color: Color = Theme.lora

    var body: some View {
        GeometryReader { g in
            ZStack(alignment: .leading) {
                Capsule().fill(Theme.s2)
                Capsule().fill(color).frame(width: g.size.width * min(max(fraction, 0), 1))
            }
        }
        .frame(height: 6)
    }
}

/// Fragments sent of a datagram, with retries.
struct FragmentProgress: View {
    let sent: Int
    let total: Int
    var retries = 0

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Bar(fraction: Double(sent) / Double(max(total, 1)))
            HStack {
                Text("Sending \(min(sent, total)) / \(total) fragments")
                Spacer()
                if retries > 0 { Text("\(retries) resent") }
            }
            .font(.system(.caption))
            .foregroundStyle(Theme.tx2)
        }
    }
}

/// Banners of the degraded states (mockup `etats`).
struct Banner: View {
    enum Kind { case sos, warn, lora, iroh, ok }

    let kind: Kind
    let title: String
    var message: String?
    var action: (label: String, run: () -> Void)?

    var body: some View {
        HStack(alignment: .center, spacing: 10) {
            Image(systemName: icon)
                .font(.system(.footnote, weight: .bold))
                .foregroundStyle(accent)
                .frame(width: 18)
                .padding(.top, 2)
                .frame(maxHeight: .infinity, alignment: .top)
            VStack(alignment: .leading, spacing: 3) {
                Text(title).font(.system(.subheadline, weight: .semibold)).foregroundStyle(accent)
                if let message {
                    Text(message).font(.system(.footnote)).foregroundStyle(Theme.tx)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            Spacer(minLength: 0)
            if let action {
                Button(action.label, action: action.run)
                    // Its own tap only, also when banners share a List row.
                    .buttonStyle(.plain)
                    .font(.system(.subheadline, weight: .semibold))
                    .padding(.horizontal, 14)
                    .frame(minHeight: 44)
                    .background(Theme.s1.opacity(0.9), in: RoundedRectangle(cornerRadius: 10))
                    .foregroundStyle(Theme.tx)
            }
        }
        .padding(12)
        .background(background, in: RoundedRectangle(cornerRadius: 14))
        .accessibilityElement(children: .combine)
    }

    private var icon: String {
        switch kind {
        case .sos, .warn: "exclamationmark.triangle"
        case .lora: "antenna.radiowaves.left.and.right"
        case .iroh: "info.circle"
        case .ok: "checkmark"
        }
    }

    private var accent: Color {
        switch kind {
        case .sos: Theme.sosFg
        case .warn: Theme.warnFg
        case .lora: Theme.loraFg
        case .iroh: Theme.irohFg
        case .ok: Theme.okFg
        }
    }

    private var background: Color {
        switch kind {
        case .sos: Theme.sosBg
        case .warn: Theme.warnBg
        case .lora: Theme.loraBg
        case .iroh: Theme.irohBg
        case .ok: Theme.okBg
        }
    }
}

/// Bars drawn from the Codec2 bits: stable and decorative.
struct Waveform: View {
    let seed: Data
    var bars = 22
    var color: Color = Theme.tx2

    var body: some View {
        HStack(alignment: .center, spacing: 2) {
            ForEach(0..<bars, id: \.self) { i in
                let b = seed.isEmpty ? 128 : Int(seed[seed.startIndex + i % seed.count])
                Capsule()
                    .fill(color)
                    .frame(width: 3, height: CGFloat(5 + (b * 7 + i * 13) % 18))
            }
        }
        .frame(height: 24)
        .accessibilityHidden(true)
    }
}

/// Hold for 3 seconds, with a circular progress ring (mockup `sos`).
struct SosHoldButton: View {
    var holdSeconds: Double = 3
    let onFire: () -> Void
    @State private var progress: Double = 0
    @State private var holding = false
    @State private var timer: Timer?

    var body: some View {
        ZStack {
            Circle().stroke(Theme.sos.opacity(0.25), lineWidth: 8)
            Circle()
                .trim(from: 0, to: progress)
                .stroke(Theme.sos, style: StrokeStyle(lineWidth: 8, lineCap: .round))
                .rotationEffect(.degrees(-90))
            Circle()
                .fill(Theme.sosBtn)
                .padding(26)
                .overlay {
                    VStack(spacing: 6) {
                        Text("SOS").font(.system(size: 52, weight: .heavy))
                        Text("Hold for 3 s").font(.system(.subheadline, weight: .medium))
                    }
                    .foregroundStyle(.white)
                }
                .scaleEffect(holding ? 0.96 : 1)
        }
        .frame(width: 240, height: 240)
        .contentShape(Circle())
        .gesture(
            DragGesture(minimumDistance: 0)
                .onChanged { _ in if !holding { start() } }
                .onEnded { _ in stop() }
        )
        .accessibilityElement()
        .accessibilityLabel("Send an SOS")
        .accessibilityHint("Hold for three seconds")
        .accessibilityAction { onFire() }
    }

    private func start() {
        holding = true
        progress = 0
        let begin = Date()
        timer = Timer.scheduledTimer(withTimeInterval: 1 / 30, repeats: true) { t in
            MainActor.assumeIsolated {
                progress = min(1, Date().timeIntervalSince(begin) / holdSeconds)
                if progress >= 1 {
                    t.invalidate()
                    holding = false
                    UINotificationFeedbackGenerator().notificationOccurred(.warning)
                    onFire()
                }
            }
        }
    }

    private func stop() {
        timer?.invalidate()
        holding = false
        if progress < 1 { withAnimation(.easeOut(duration: 0.2)) { progress = 0 } }
    }
}

/// One measurement in search mode: who heard, RSSI, SNR, bearing, age.
struct MeasurementRow: View {
    let who: String
    let detail: String
    let value: String

    var body: some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(who).font(.system(.callout, weight: .semibold))
                Text(detail).font(.system(.footnote)).foregroundStyle(Theme.tx2)
            }
            Spacer()
            Text(value).font(Theme.figure(17))
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }
}

/// A key fingerprint: 6 groups of 4 hex digits, 3 per line.
struct KeyFingerprint: View {
    let groups: [String]
    /// For UI tests: "fingerprint", or "safetyNumber".
    var identifier = "fingerprint"

    var body: some View {
        Grid(horizontalSpacing: 28, verticalSpacing: 8) {
            ForEach(Array(stride(from: 0, to: groups.count, by: 3)), id: \.self) { row in
                GridRow {
                    ForEach(row..<min(row + 3, groups.count), id: \.self) { i in
                        Text(groups[i]).font(Theme.figure(22))
                    }
                }
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier(identifier)
    }
}

/// A safety number: 12 groups of 5 digits, 3 per line (ADR-0029).
struct SafetyNumber: View {
    let groups: [String]

    var body: some View {
        KeyFingerprint(groups: groups, identifier: "safetyNumber")
    }
}

/// A row's icon in a 30 pt tile: neutral (`s2` with `tx`), or tinted for LoRa, iroh and Tor.
/// Hidden from VoiceOver: the row's title says what it is; left out at the accessibility text
/// sizes, where the row's words need the room.
struct IconTile: View {
    enum Tint { case neutral, lora, iroh, tor }

    let symbol: String
    var tint = Tint.neutral
    var side: CGFloat = 30
    @ScaledMetric(relativeTo: .callout) private var scale: CGFloat = 1
    @Environment(\.dynamicTypeSize) private var textSize

    var body: some View {
        let side = self.side * scale
        if !textSize.isAccessibilitySize {
            tile(side)
        }
    }

    private func tile(_ side: CGFloat) -> some View {
        Image(systemName: symbol)
            .font(.system(size: side * 0.5, weight: .semibold))
            .foregroundStyle(colours.1)
            .frame(width: side, height: side)
            .background(colours.0, in: RoundedRectangle(cornerRadius: side * 0.27))
            .accessibilityHidden(true)
    }

    private var colours: (Color, Color) {
        switch tint {
        case .neutral: (Theme.s2, Theme.tx)
        case .lora: (Theme.loraBg, Theme.loraFg)
        case .iroh: (Theme.irohBg, Theme.irohFg)
        case .tor: (Theme.torBg, Theme.torFg)
        }
    }
}

/// A row inside a card: an icon tile, title, subtitle, trailing content.
struct CardRow<Trailing: View>: View {
    let title: String
    var subtitle: String?
    var icon: String?
    var tint = IconTile.Tint.neutral
    @ViewBuilder var trailing: () -> Trailing

    var body: some View {
        HStack(spacing: 12) {
            if let icon { IconTile(symbol: icon, tint: tint) }
            // Its control beside the words while they keep half the row, else under them.
            BesideOrUnder {
                VStack(alignment: .leading, spacing: 2) {
                    // Whole on more lines with larger text, beside its tile.
                    Text(title).font(.system(.callout, weight: .medium)).foregroundStyle(Theme.tx)
                        .fixedSize(horizontal: false, vertical: true)
                    if let subtitle {
                        Text(subtitle).font(.system(.footnote)).foregroundStyle(Theme.tx2)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                trailing()
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 11)
        .frame(minHeight: 44)
    }
}

extension CardRow where Trailing == EmptyView {
    init(title: String, subtitle: String? = nil, icon: String? = nil, tint: IconTile.Tint = .neutral) {
        self.init(title: title, subtitle: subtitle, icon: icon, tint: tint) { EmptyView() }
    }
}

/// A node's MeshCore public key (F-1): its 64 hex digits in groups of 8, as the stock MeshCore
/// app writes them; a tap copies them ("Copied").
struct MeshCoreKeyRow: View {
    let key: Data
    /// The row's icon tile, in a card of tiled rows.
    var icon: String?
    @State private var copied = false

    /// "1a2b3c4d 5e6f7a8b …": 8 groups of 8 digits, 4 to a line.
    static func grouped(_ key: Data) -> String {
        let hex = Array(key.hex)
        let groups = stride(from: 0, to: hex.count, by: 8).map { String(hex[$0..<min($0 + 8, hex.count)]) }
        return stride(from: 0, to: groups.count, by: 4)
            .map { groups[$0..<min($0 + 4, groups.count)].joined(separator: " ") }
            .joined(separator: "\n")
    }

    var body: some View {
        Button {
            UIPasteboard.general.string = key.hex
            copied = true
            DispatchQueue.main.asyncAfter(deadline: .now() + 2) { copied = false }
        } label: {
            HStack(alignment: .top, spacing: 12) {
                if let icon { IconTile(symbol: icon) }
                VStack(alignment: .leading, spacing: 4) {
                    Text("MeshCore public key").font(.system(.callout, weight: .medium)).foregroundStyle(Theme.tx)
                    Text(Self.grouped(key))
                        .font(.system(.footnote, design: .monospaced))
                        .foregroundStyle(Theme.tx2)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 8)
                Text(copied ? String(localized: "Copied") : String(localized: "Copy"))
                    .font(.system(.footnote, weight: .semibold))
                    .foregroundStyle(copied ? Theme.ok : Theme.tx2)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 11)
            .frame(minHeight: 44)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(String(localized: "MeshCore public key"))
        .accessibilityValue(copied ? String(localized: "Copied") : key.hex)
        .accessibilityHint(String(localized: "Copies it"))
        .accessibilityIdentifier("meshcoreKey")
    }
}

/// Rows in a card; put a `CardDivider` between them.
struct CardList<Content: View>: View {
    @ViewBuilder var content: () -> Content

    var body: some View {
        VStack(spacing: 0, content: content)
            .background(Theme.s1, in: RoundedRectangle(cornerRadius: 14))
    }
}

/// The line between two rows of a card.
struct CardDivider: View {
    var inset: CGFloat = 16
    /// Between rows with an icon tile: from the titles' edge (the tiles are left out at the
    /// accessibility text sizes).
    var tiled = false
    @Environment(\.dynamicTypeSize) private var textSize

    static var tiled: CardDivider { CardDivider(tiled: true) }

    var body: some View {
        Rectangle().fill(Theme.line).frame(height: 1)
            .padding(.leading, tiled && !textSize.isAccessibilitySize ? 58 : inset)
    }
}

/// A placeholder for screens of a later milestone.
struct LaterNote: View {
    let text: LocalizedStringKey

    var body: some View {
        Label(text, systemImage: "clock")
            .font(.system(.caption))
            .foregroundStyle(Theme.tx2)
    }
}

func seconds(_ ms: UInt64) -> String { String(ms / 1000) }

/// "62.5 kHz", "62,5 kHz" in French.
/// A decimal in the app's language, "869,618" in French (B-ios-26), as Android writes them
/// (B-and-25). Coordinates keep the point: figures any map and any reader take (B-and-20).
func decimal(_ x: Double, _ digits: Int) -> String {
    x.formatted(.number.precision(.fractionLength(digits)).grouping(.never))
}

func bandwidth(_ hz: UInt32) -> String {
    (Double(hz) / 1000).formatted(.number.precision(.fractionLength(0...1))) + " kHz"
}

// MARK: - a node's signal (ADR-0049)

extension SignalRating {
    /// `ok` Good, `warn` Fair, `sos` Bad; `line` without a reading.
    var color: Color {
        switch self {
        case .good: Theme.ok
        case .fair: Theme.warn
        case .bad: Theme.sos
        case .none: Theme.line
        }
    }

    /// "Signal good, 7.5 dB", as VoiceOver reads a meter; nil without a reading.
    func said(_ snr: Float?) -> String? {
        guard let snr else { return nil }
        switch self {
        case .good: return String(localized: "Signal good, \(dbText(snr))")
        case .fair: return String(localized: "Signal fair, \(dbText(snr))")
        case .bad: return String(localized: "Signal weak, \(dbText(snr))")
        case .none: return nil
        }
    }
}

/// A node's signal: three bars (Good 3 in `ok`, Fair 2 in `warn`, Bad 1 in `sos`, the rest in
/// `line`) over the SNR in tabular digits; nothing without a reading. Large on a node's page,
/// small in a row.
struct SignalMeter: View {
    let rating: SignalRating
    let snr: Float?
    var large = false
    @ScaledMetric(relativeTo: .footnote) private var scale: CGFloat = 1

    var body: some View {
        if let said = rating.said(snr), let snr {
            let lit = rating == .good ? 3 : rating == .fair ? 2 : 1
            let unit = (large ? 7 : 4.6) * scale
            VStack(alignment: .trailing, spacing: large ? 4 : 3) {
                HStack(alignment: .bottom, spacing: large ? 3 : 2) {
                    ForEach(1...3, id: \.self) { i in
                        RoundedRectangle(cornerRadius: 1.5)
                            .fill(i <= lit ? rating.color : Theme.line)
                            .frame(width: (large ? 6 : 4) * scale, height: unit * (CGFloat(i) + 0.3))
                    }
                }
                Text(verbatim: dbText(snr))
                    .font(.system(large ? .subheadline : .footnote, weight: .semibold).monospacedDigit())
                    .foregroundStyle(Theme.tx)
                    .fixedSize()
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(said)
        }
    }
}

/// "1 hop", "2 hops", "path unknown": how far a node is through repeaters.
struct HopPill: View {
    let hops: UInt8?

    var body: some View {
        Text(text)
            .font(.system(.caption, weight: .semibold))
            .foregroundStyle(Theme.tx2)
            .fixedSize()
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(Theme.s2, in: RoundedRectangle(cornerRadius: 8))
    }

    private var text: String {
        switch hops {
        case nil: String(localized: "path unknown")
        case 0?: String(localized: "direct")
        case let h?: String(localized: "\(Int(h)) hops")
        }
    }
}

/// "lichen", with a lock: a node whose identity is known (a lichen user).
struct LichenBadge: View {
    var body: some View {
        HStack(spacing: 3) {
            Image(systemName: "lock").font(.system(.caption2, weight: .semibold)).accessibilityHidden(true)
            // Caption, not caption2: caption2 stays 11 pt at the smaller text sizes.
            Text(verbatim: "lichen").font(.system(.caption, weight: .semibold))
        }
        .foregroundStyle(Theme.tx2)
        .fixedSize()
        .padding(.horizontal, 6)
        .padding(.vertical, 2)
        .background(Theme.s2, in: RoundedRectangle(cornerRadius: 6))
    }
}

/// A view and what goes at its end (a figure, a control, a button): beside it while the view
/// keeps at least half the row, else under it, at its start (larger text, French). Without a
/// second view, the first alone.
struct BesideOrUnder: Layout {
    var spacing: CGFloat = 8
    var below: CGFloat = 6
    /// Beside: right after the first view (a badge after a name), not at the row's end.
    var packed = false

    private func beside(_ subviews: Subviews, _ width: CGFloat) -> Bool {
        guard subviews.count > 1 else { return true }
        let end = subviews[1].sizeThatFits(.unspecified).width
        return width - end - spacing >= width / 2
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        guard let first = subviews.first else { return .zero }
        guard subviews.count > 1 else { return first.sizeThatFits(proposal) }
        let end = subviews[1].sizeThatFits(.unspecified)
        guard let width = proposal.width else {
            let lead = first.sizeThatFits(.unspecified)
            return CGSize(width: lead.width + spacing + end.width, height: max(lead.height, end.height))
        }
        if beside(subviews, width) {
            let lead = first.sizeThatFits(ProposedViewSize(width: width - end.width - spacing, height: nil))
            return CGSize(width: width, height: max(lead.height, end.height))
        }
        let lead = first.sizeThatFits(ProposedViewSize(width: width, height: nil))
        let under = subviews[1].sizeThatFits(ProposedViewSize(width: min(end.width, width), height: nil))
        return CGSize(width: width, height: lead.height + below + under.height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        guard let first = subviews.first else { return }
        guard subviews.count > 1 else {
            first.place(at: bounds.origin, proposal: ProposedViewSize(width: bounds.width, height: bounds.height))
            return
        }
        let end = subviews[1].sizeThatFits(.unspecified)
        if beside(subviews, bounds.width) {
            let room = bounds.width - end.width - spacing
            let lead = first.sizeThatFits(ProposedViewSize(width: room, height: nil))
            first.place(at: CGPoint(x: bounds.minX, y: bounds.midY), anchor: .leading,
                        proposal: ProposedViewSize(width: room, height: lead.height))
            if packed {
                subviews[1].place(at: CGPoint(x: bounds.minX + min(lead.width, room) + spacing, y: bounds.midY),
                                  anchor: .leading, proposal: .unspecified)
            } else {
                subviews[1].place(at: CGPoint(x: bounds.maxX, y: bounds.midY), anchor: .trailing, proposal: .unspecified)
            }
        } else {
            let whole = ProposedViewSize(width: bounds.width, height: nil)
            let lead = first.sizeThatFits(whole)
            first.place(at: bounds.origin, proposal: whole)
            subviews[1].place(at: CGPoint(x: bounds.minX, y: bounds.minY + lead.height + below),
                              proposal: ProposedViewSize(width: min(end.width, bounds.width), height: nil))
        }
    }
}

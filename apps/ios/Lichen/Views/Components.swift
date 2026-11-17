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
    }
}

/// A safety number: 12 groups of 5 digits, 3 per line (ADR-0029).
struct SafetyNumber: View {
    let groups: [String]

    var body: some View {
        KeyFingerprint(groups: groups)
    }
}

/// A row inside a card: title, subtitle, trailing content.
struct CardRow<Trailing: View>: View {
    let title: String
    var subtitle: String?
    @ViewBuilder var trailing: () -> Trailing

    var body: some View {
        HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.system(.callout, weight: .medium)).foregroundStyle(Theme.tx)
                if let subtitle {
                    Text(subtitle).font(.system(.footnote)).foregroundStyle(Theme.tx2)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            Spacer(minLength: 8)
            trailing()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 11)
        .frame(minHeight: 44)
    }
}

extension CardRow where Trailing == EmptyView {
    init(title: String, subtitle: String? = nil) {
        self.init(title: title, subtitle: subtitle) { EmptyView() }
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

    var body: some View {
        Rectangle().fill(Theme.line).frame(height: 1).padding(.leading, inset)
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
func bandwidth(_ hz: UInt32) -> String {
    (Double(hz) / 1000).formatted(.number.precision(.fractionLength(0...1))) + " kHz"
}

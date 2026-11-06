import SwiftUI

/// The chirp mark (docs/brand/lichen-mark.svg), drawn on its 64-unit grid.
struct ChirpMark: View {
    var color: Color = Theme.lora

    var body: some View {
        Canvas { ctx, size in
            let s = min(size.width, size.height) / 64
            var p = Path()
            p.move(to: CGPoint(x: 10 * s, y: 50 * s))
            for (x, y) in [(24.0, 14.0), (24, 50), (38, 14), (38, 50), (48, 28.3)] {
                p.addLine(to: CGPoint(x: x * s, y: y * s))
            }
            ctx.stroke(p, with: .color(color), style: StrokeStyle(lineWidth: 5.5 * s, lineCap: .round, lineJoin: .round))
            ctx.fill(Path(ellipseIn: CGRect(x: 49.5 * s, y: 12.4 * s, width: 8 * s, height: 8 * s)), with: .color(color))
        }
        .aspectRatio(1, contentMode: .fit)
        .accessibilityHidden(true)
    }
}

/// First launch: welcome, then pairing (mockups `accueil`, `appairage`).
struct OnboardingView: View {
    let done: () -> Void

    var body: some View {
        NavigationStack {
            WelcomeView(done: done)
        }
    }
}

private struct WelcomeView: View {
    let done: () -> Void
    @Environment(AppModel.self) private var model
    @State private var pairing = false

    var body: some View {
        VStack(spacing: 0) {
            Spacer().frame(height: 60)
            VStack(spacing: 20) {
                ChirpMark()
                    .frame(width: 78, height: 78)
                    .frame(width: 120, height: 120)
                    .background(Theme.s1, in: RoundedRectangle(cornerRadius: 27))
                    .overlay(RoundedRectangle(cornerRadius: 27).stroke(Theme.line))
                Text(verbatim: "lichen")
                    .font(Theme.wordmark(48))
                    .tracking(-1.5)
                Text("Encrypted messaging off the grid, over LoRa and beyond.")
                    .font(.system(size: 17))
                    .foregroundStyle(Theme.tx2)
                    .multilineTextAlignment(.center)
            }
            Spacer()
            VStack(alignment: .leading, spacing: 16) {
                point("No cell towers", "Messages go from radio to radio.")
                point("End-to-end encrypted", "Verified keys, nothing in clear on the air.")
                point("Find your people", "Location sharing and search mode.")
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            Spacer()
            VStack(spacing: 8) {
                Button("Connect my radio") { pairing = true }
                    .buttonStyle(.primary)
                Button("Later", action: done)
                    .font(.system(size: 16, weight: .semibold))
                    .frame(maxWidth: .infinity, minHeight: 50)
                    .foregroundStyle(Theme.tx)
            }
        }
        .padding(.horizontal, 24)
        .padding(.bottom, 12)
        .screen()
        .toolbar(.hidden, for: .navigationBar)
        .navigationDestination(isPresented: $pairing) { PairingView(done: done) }
        .onAppear {
            #if DEBUG
            if model.debugScreen == "pairing" { pairing = true }
            #endif
        }
    }

    private func point(_ title: LocalizedStringKey, _ detail: LocalizedStringKey) -> some View {
        HStack(alignment: .top, spacing: 14) {
            Circle().fill(Theme.lora).frame(width: 8, height: 8).padding(.top, 7)
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.system(size: 16, weight: .semibold))
                Text(detail).font(.system(size: 14)).foregroundStyle(Theme.tx2)
            }
        }
    }
}

/// Bluetooth scan and pairing. The 6-digit code shown on the radio is typed in iOS's own
/// pairing prompt, which the app cannot replace.
struct PairingView: View {
    var done: (() -> Void)?
    @Environment(AppModel.self) private var model

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                Text("Turn your radio on and keep it close to the phone. Stock MeshCore companion firmware: nothing to flash.")
                    .font(.system(size: 15))
                    .foregroundStyle(Theme.tx2)
                HStack(spacing: 8) {
                    Image(systemName: "dot.radiowaves.left.and.right")
                    Text(scanLine)
                }
                .font(.system(size: 13))
                .foregroundStyle(Theme.tx2)
                if !model.found.isEmpty {
                    CardList {
                        ForEach(Array(model.found.enumerated()), id: \.element.id) { i, f in
                            if i > 0 { CardDivider() }
                            radioRow(f)
                        }
                    }
                }
                VStack(alignment: .leading, spacing: 10) {
                    Text("Code shown on the radio").font(.system(size: 17, weight: .semibold))
                    Text("When you pair, iOS asks for the 6 digits shown on the radio's screen: type them in its prompt. The radio then stays paired with this phone.")
                        .font(.system(size: 14))
                        .foregroundStyle(Theme.tx2)
                        .fixedSize(horizontal: false, vertical: true)
                    HStack(spacing: 8) {
                        ForEach(0..<6, id: \.self) { _ in
                            RoundedRectangle(cornerRadius: 10)
                                .fill(Theme.s2)
                                .overlay(RoundedRectangle(cornerRadius: 10).stroke(Theme.line))
                                .frame(width: 44, height: 52)
                        }
                    }
                    .frame(maxWidth: .infinity)
                    .accessibilityHidden(true)
                }
                .card(padding: 16)
                .padding(.top, 8)
                if let done {
                    Button("Try the demo network instead") {
                        model.startDemo()
                        done()
                    }
                    .buttonStyle(.secondary)
                    .padding(.top, 4)
                }
            }
            .padding(16)
        }
        .screen()
        .navigationTitle("Pair a radio")
        .navigationBarTitleDisplayMode(.inline)
        .toolbarBackground(Theme.s1, for: .navigationBar)
        .toolbarBackground(.visible, for: .navigationBar)
        .onAppear { model.companion.scan() }
        .onDisappear { model.companion.stopScan() }
        .onChange(of: model.isConnected) {
            if model.isConnected { done?() }
        }
    }

    private var scanLine: String {
        switch model.companionState {
        case .off: String(localized: "Bluetooth is off")
        case let .connecting(name): String(localized: "Connecting to \(name)…")
        case let .connected(name): String(localized: "\(name) connected")
        default: String(localized: "Searching over Bluetooth…")
        }
    }

    private func radioRow(_ f: Companion.Found) -> some View {
        HStack(spacing: 12) {
            Image(systemName: "antenna.radiowaves.left.and.right")
                .foregroundStyle(Theme.lora)
                .frame(width: 40, height: 40)
                .background(Theme.s2, in: RoundedRectangle(cornerRadius: 10))
            VStack(alignment: .leading, spacing: 2) {
                Text(f.name).font(.system(size: 16))
                Text("\(f.rssi) dBm · MeshCore companion").font(.system(size: 13)).foregroundStyle(Theme.tx2)
            }
            Spacer()
            Button("Pair") { model.companion.connect(f.id) }
                .buttonStyle(SecondaryButtonStyle(height: 40))
                .frame(width: 96)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 11)
    }
}

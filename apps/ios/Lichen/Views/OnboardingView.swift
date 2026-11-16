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
    @State private var identity = false
    @State private var contact = false
    @State private var restoring = false

    var body: some View {
        // Fits on one screen when it can; scrolls at large text sizes (audit M12).
        ViewThatFits(in: .vertical) {
            content
            ScrollView { content }
        }
        .padding(.horizontal, 24)
        .padding(.bottom, 12)
        .screen()
        .toolbar(.hidden, for: .navigationBar)
        .sheet(isPresented: $restoring) { RestoreBackupSheet(restored: done) }
        .navigationDestination(isPresented: $pairing) { PairingView(done: { identity = true }) }
        .navigationDestination(isPresented: $identity) {
            IdentityView(next: { contact = true })
                .navigationDestination(isPresented: $contact) { AddContactView(next: done) }
        }
        .onAppear {
            #if DEBUG
            if model.debugScreen == "pairing" { pairing = true }
            #endif
        }
    }

    private var content: some View {
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
                    .font(.system(.body))
                    .foregroundStyle(Theme.tx2)
                    .multilineTextAlignment(.center)
            }
            Spacer()
            VStack(alignment: .leading, spacing: 16) {
                point("No cell towers", "Messages go from radio to radio.")
                point("End-to-end encrypted", "Only you and the people you write to can read your messages. Check their key in person.")
                point("Find your people", "Location sharing and search mode.")
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            Spacer()
            VStack(spacing: 8) {
                Button("Connect my radio") { pairing = true }
                    .buttonStyle(.primary)
                Button("Later") { identity = true }
                    .font(.system(.callout, weight: .semibold))
                    .frame(maxWidth: .infinity, minHeight: 50)
                    .foregroundStyle(Theme.tx)
                // A phone that replaces another (ADR-0037): everything comes back from the file.
                Button("Restore a backup") { restoring = true }
                    .font(.system(.footnote, weight: .medium))
                    .frame(maxWidth: .infinity, minHeight: 44)
                    .foregroundStyle(Theme.tx2)
            }
        }
    }

    private func point(_ title: LocalizedStringKey, _ detail: LocalizedStringKey) -> some View {
        HStack(alignment: .top, spacing: 14) {
            Circle().fill(Theme.lora).frame(width: 8, height: 8).padding(.top, 7)
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.system(.callout, weight: .semibold))
                Text(detail).font(.system(.subheadline)).foregroundStyle(Theme.tx2)
            }
        }
    }
}

/// Bluetooth scan and pairing. The 6-digit code shown on the radio is typed in iOS's own
/// pairing prompt, which the app cannot replace.
struct PairingView: View {
    var done: (() -> Void)?
    @Environment(AppModel.self) private var model
    @State private var address = TcpCompanion.lastTyped

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                Text("Turn your radio on and keep it close to the phone. Stock MeshCore companion firmware: nothing to flash.")
                    .font(.system(.subheadline))
                    .foregroundStyle(Theme.tx2)
                HStack(spacing: 8) {
                    Image(systemName: "dot.radiowaves.left.and.right")
                    Text(scanLine)
                }
                .font(.system(.footnote))
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
                    Text("Code shown on the radio").font(.system(.body, weight: .semibold))
                    Text("When you pair, iOS asks for the 6 digits shown on the radio's screen: type them in its prompt. The radio then stays paired with this phone.")
                        .font(.system(.subheadline))
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
                network
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

    /// A companion on Wi-Fi, or a USB radio relayed by `lichen bridge`; the link is clear and
    /// unauthenticated, and the screen says so (review of 2026-11-15, SR-31).
    @ViewBuilder private var network: some View {
        SectionLabel("Network").padding(.top, 8)
        Text("A companion on Wi-Fi, or a USB radio relayed by a computer (lichen bridge; the simulator reaches it at 127.0.0.1:5000). The network link to the radio is not encrypted: on a shared network, someone can read and change MeshCore chats, contacts and channels. lichen messages stay end-to-end encrypted.")
            .font(.system(.subheadline))
            .foregroundStyle(Theme.tx2)
            .fixedSize(horizontal: false, vertical: true)
        HStack(spacing: 8) {
            TextField("host:port", text: $address)
                .accessibilityIdentifier("tcpAddress")
                .keyboardType(.URL)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .font(.system(.callout, design: .monospaced))
                .padding(12)
                .background(Theme.s1, in: RoundedRectangle(cornerRadius: 12))
                .overlay(RoundedRectangle(cornerRadius: 12).stroke(Theme.line))
                .onSubmit { model.connectTcp(address) }
            Button("Connect") { model.connectTcp(address) }
                .buttonStyle(SecondaryButtonStyle(height: 46))
                .frame(width: 120)
                .disabled(tcpAddress(text: address) == nil)
        }
        if let error = model.tcpError {
            Text("Could not reach the radio: \(error)")
                .font(.system(.footnote))
                .foregroundStyle(Theme.danger)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private func radioRow(_ f: Companion.Found) -> some View {
        HStack(spacing: 12) {
            Image(systemName: "antenna.radiowaves.left.and.right")
                .foregroundStyle(Theme.lora)
                .frame(width: 40, height: 40)
                .background(Theme.s2, in: RoundedRectangle(cornerRadius: 10))
            VStack(alignment: .leading, spacing: 2) {
                Text(f.name).font(.system(.callout))
                Text("\(f.rssi) dBm · MeshCore companion").font(.system(.footnote)).foregroundStyle(Theme.tx2)
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

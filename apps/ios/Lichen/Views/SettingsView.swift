import SwiftUI

/// Node pairing and status, airtime budget.
struct SettingsView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    if model.isConnected {
                        LabeledContent("Name", value: model.nodeName)
                        if let k = model.nodeKey {
                            LabeledContent("Key", value: k.prefix(8).hex)
                        }
                        if let r = model.radio {
                            LabeledContent("Radio", value: "\(String(format: "%.3f", Double(r.freqKhz) / 1000)) MHz · \(bandwidth(r.bwHz)) · SF\(r.sf) · CR 4/\(r.cr)")
                        }
                        LabeledContent("End-to-end encryption", value: identityStatus)
                        if model.isDemo {
                            Button("Leave the demo", role: .destructive) { model.stopDemo() }
                        } else {
                            Button("Disconnect", role: .destructive) { model.companion.disconnect() }
                        }
                    } else {
                        Button(isScanning ? "Stop looking" : "Find nodes") {
                            if isScanning { model.companion.stopScan() } else { model.companion.scan() }
                        }
                        ForEach(model.found) { f in
                            Button { model.companion.connect(f.id) } label: {
                                HStack {
                                    Text(f.name).foregroundStyle(.primary)
                                    Spacer()
                                    Text("\(String(f.rssi)) dBm").foregroundStyle(Theme.secondaryText)
                                }
                            }
                        }
                    }
                } header: {
                    Text("MeshCore node")
                } footer: {
                    Text("Pairing asks for the PIN shown on the node. Messages arrive only while the phone is connected to it.")
                }
                if !model.isConnected {
                    Section {
                        Button("Try the demo network") { model.startDemo() }
                    } footer: {
                        Text("A simulated MeshCore network with two other users and a group, in real time: the app's real core, without a radio.")
                    }
                }
                Section {
                    LabeledContent("Used this hour", value: "\(seconds(model.budget.usedMs)) s / \(seconds(model.budget.limitMs)) s")
                } header: {
                    Text("Airtime")
                } footer: {
                    Text("EU rules cap each node's time on air. lichen counts its own transmissions and waits when the budget is spent.")
                }
                Section("About") {
                    LabeledContent("Version", value: Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "")
                }
            }
            .navigationTitle("Settings")
        }
    }

    private var isScanning: Bool { model.companionState == .scanning }

    private var identityStatus: String {
        if model.keyExportRefused { return String(localized: "Key export refused by the node") }
        return model.identityReady ? String(localized: "Ready") : String(localized: "Waiting for the node's key")
    }
}

struct MapPlaceholderView: View {
    var body: some View {
        ContentUnavailableView(
            "Map and location sharing",
            systemImage: "map",
            description: Text("Coming with milestone L5. Your position is never put in MeshCore adverts.")
        )
    }
}

struct SearchPlaceholderView: View {
    var body: some View {
        ContentUnavailableView(
            "Search mode",
            systemImage: "scope",
            description: Text("Coming with milestone L8: find a group member with RSSI, bearings and SOS beacons.")
        )
    }
}

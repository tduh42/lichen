import SwiftUI

@main
struct LichenApp: App {
    @State private var model = AppModel()

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(model)
                .tint(Theme.accent)
        }
    }
}

struct RootView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        @Bindable var model = model
        TabView(selection: $model.tab) {
            ConversationsView()
                .tabItem { Label("Messages", systemImage: "bubble.left") }
                .tag(AppModel.Tab.messages)
            MapScreen()
                .tabItem { Label("Map", systemImage: "map") }
                .tag(AppModel.Tab.map)
            SearchPlaceholderView()
                .tabItem { Label("Search", systemImage: "scope") }
                .tag(AppModel.Tab.search)
            SettingsView()
                .tabItem { Label("Settings", systemImage: "gearshape") }
                .tag(AppModel.Tab.settings)
        }
        .overlay(alignment: .top) {
            if let sos = model.incomingSos, model.tab != .map {
                SosBanner(name: model.name(of: sos.from)) {
                    model.tab = .map
                } dismiss: {
                    model.incomingSos = nil
                }
                .padding(.horizontal, 16)
                .transition(.move(edge: .top).combined(with: .opacity))
            }
        }
        .animation(.snappy, value: model.incomingSos?.from)
    }
}

/// Someone sent an SOS: shown over every tab but the map.
private struct SosBanner: View {
    let name: String
    let show: () -> Void
    let dismiss: () -> Void

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: "sos")
                .font(.title3.weight(.bold))
            VStack(alignment: .leading, spacing: 2) {
                Text("SOS from \(name)")
                    .font(.headline)
                Text("Their exact position is on the map.")
                    .font(.caption)
            }
            Spacer()
            Button("Show", action: show)
                .buttonStyle(.borderedProminent)
                .tint(.white)
                .foregroundStyle(Theme.danger)
            Button(action: dismiss) { Image(systemName: "xmark") }
                .accessibilityLabel("Dismiss")
        }
        .foregroundStyle(.white)
        .padding(14)
        .background(Theme.danger, in: RoundedRectangle(cornerRadius: 16))
        .shadow(radius: 8)
    }
}

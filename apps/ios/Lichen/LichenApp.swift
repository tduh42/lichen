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
    var body: some View {
        TabView {
            ConversationsView()
                .tabItem { Label("Messages", systemImage: "bubble.left") }
            MapPlaceholderView()
                .tabItem { Label("Map", systemImage: "map") }
            SearchPlaceholderView()
                .tabItem { Label("Search", systemImage: "scope") }
            SettingsView()
                .tabItem { Label("Settings", systemImage: "gearshape") }
        }
    }
}

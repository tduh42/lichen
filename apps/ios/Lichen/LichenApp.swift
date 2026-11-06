import SwiftUI

@main
struct LichenApp: App {
    @State private var model = AppModel()
    @AppStorage("appearance") private var appearance = Appearance.system

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(model)
                .tint(Theme.tx)
                .preferredColorScheme(appearance.scheme)
        }
    }
}

struct RootView: View {
    @Environment(AppModel.self) private var model
    @AppStorage("onboarded") private var onboarded = false

    var body: some View {
        @Bindable var model = model
        Group {
            if onboarded {
                tabs
            } else {
                OnboardingView { onboarded = true }
            }
        }
        #if DEBUG
        // -screen identity: the identity screen, for screenshots.
        .sheet(isPresented: .constant(model.debugScreen == "identity")) {
            NavigationStack { IdentityView() }
        }
        #endif
        .fullScreenCover(item: $model.sosAlert) { sos in
            SosReceivedView(sos: sos)
        }
        .onAppear {
            #if DEBUG
            if UserDefaults.standard.bool(forKey: "onboarding") { onboarded = false }
            if UserDefaults.standard.bool(forKey: "demo") { onboarded = true }
            #endif
        }
    }

    private var tabs: some View {
        @Bindable var model = model
        return TabView(selection: $model.tab) {
            ConversationsView()
                .tabItem { Label("Messages", systemImage: "bubble.left") }
                .tag(AppModel.Tab.messages)
                .badge(model.totalUnread)
            MapScreen()
                .tabItem { Label("Map", systemImage: "map") }
                .tag(AppModel.Tab.map)
            SearchView()
                .tabItem { Label("Search", systemImage: "scope") }
                .tag(AppModel.Tab.search)
                .badge(model.incomingSos == nil ? 0 : 1)
            SettingsView()
                .tabItem { Label("Settings", systemImage: "gearshape") }
                .tag(AppModel.Tab.settings)
        }
    }
}

extension PeerPosition: Identifiable {
    public var id: Data { from }
}

import SwiftUI

@main
struct LichenApp: App {
    @State private var model: AppModel
    @AppStorage("appearance") private var appearance = Appearance.system

    init() {
        #if DEBUG
        AppModel.debugFreshInstall()
        #endif
        _model = State(initialValue: AppModel())
    }

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
    @Environment(\.scenePhase) private var scenePhase
    @AppStorage("onboarded") private var onboarded = false

    var body: some View {
        @Bindable var model = model
        Group {
            if let problem = model.identityProblem {
                IdentityProblemView(problem: problem)
            } else if onboarded {
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
        .fullScreenCover(item: $model.sosAlert) { alert in
            SosReceivedView(from: alert.id)
        }
        // "Open in lichen" from another app (ADR-0034): a file or a photo, then where to send it.
        .onOpenURL { model.receiveShared($0) }
        .sheet(item: $model.incomingShare) { ShareTargetSheet(shared: $0) }
        .alert(model.shareError ?? "", isPresented: Binding(get: { model.shareError != nil }, set: { if !$0 { model.shareError = nil } })) {
            Button("OK", role: .cancel) {}
        }
        // The app lock (Settings › Privacy): its own window covers everything, sheets included,
        // and shows the SOS alert's line; this one covers the app's window until it is up.
        .overlay {
            if model.lock.covered { LockCover() }
        }
        .onAppear { model.lock.attach { [model] in AnyView(LockCover().environment(model)) } }
        .onChange(of: scenePhase, initial: true) { _, phase in
            model.lock.scene(phase)
            model.scene(phase)
        }
        // Notifications for messages and SOS: asked once the welcome screens are done.
        .task(id: onboarded) { if onboarded { model.notifier.requestAuthorization() } }
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
                .tabItem { Label("Locate", systemImage: "scope") }
                .tag(AppModel.Tab.search)
                .badge(model.activeIncidents.count)
            SettingsView()
                .tabItem { Label("Settings", systemImage: "gearshape") }
                .tag(AppModel.Tab.settings)
        }
    }
}

extension PeerPosition: Identifiable {
    public var id: Data { from }
}

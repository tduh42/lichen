import LocalAuthentication
import SwiftUI
import UIKit

/// The app lock (Settings › Privacy): Face ID, Touch ID or the passcode before lichen shows
/// anything, on a cold start and when it comes back after `delay` in the background. App only:
/// the core, the radio link, the outbox and notifications go on behind the cover. While the lock
/// is on, the cover also hides the app switcher's snapshot, and message notifications say only
/// "New message" (`Notifier`); SOS notifications keep their content.
///
/// The cover is a window of its own above the app's, so it hides sheets and full-screen covers
/// too; `RootView` draws the same cover in the app's window for the frames before it is up.
@MainActor @Observable
final class AppLock {
    static let enabledKey = "lichen.lock"
    static let delayKey = "lichen.lockAfter"
    /// What "Lock after" offers, in seconds: immediately, 1 minute, 15 minutes, 1 hour.
    static let delays = [0, 60, 900, 3_600]

    /// Message notifications hide their sender and text: the lock is on.
    static var hidesNotifications: Bool { UserDefaults.standard.bool(forKey: enabledKey) }

    /// The lock is on (Settings); turned on only after one successful authentication.
    private(set) var enabled = UserDefaults.standard.bool(forKey: enabledKey) {
        didSet {
            UserDefaults.standard.set(enabled, forKey: Self.enabledKey)
            update()
        }
    }

    /// Seconds in the background before lichen locks.
    var delay = UserDefaults.standard.integer(forKey: delayKey) {
        didSet { UserDefaults.standard.set(delay, forKey: Self.delayKey) }
    }

    /// The cover is up until the owner authenticates. A cold start begins locked.
    private(set) var locked = UserDefaults.standard.bool(forKey: enabledKey) {
        didSet { update() }
    }

    /// The scene is not active: app switcher, Control Center, a system prompt, the background.
    private var inactive = false {
        didSet { update() }
    }

    /// A prompt of ours is up, or just went: the scene is inactive for it, and the cover does
    /// not need to hide the app from the owner. Cleared when the scene is active again.
    private var prompting = false {
        didSet { update() }
    }

    /// The system prompt is up: the Unlock button waits.
    private(set) var authenticating = false

    /// Why the lock could not be turned on.
    var problem: String?

    /// Which biometry the device has, for the switch's subtitle.
    let biometry: LABiometryType = {
        let context = LAContext()
        _ = context.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: nil)
        return context.biometryType
    }()

    @ObservationIgnored private var backgroundSince: ContinuousClock.Instant?
    /// The prompt came up by itself for this lock (once).
    @ObservationIgnored private var prompted = false
    @ObservationIgnored private var window: UIWindow?
    @ObservationIgnored private weak var appWindow: UIWindow?
    @ObservationIgnored private var cover: (() -> AnyView)?

    /// The cover hides the app: locked, or the lock is on and the scene is not active.
    var covered: Bool { locked || (enabled && inactive && !prompting) }

    /// What the cover window shows (`LockCover` with the model in its environment).
    func attach(cover: @escaping () -> AnyView) {
        self.cover = cover
        update()
    }

    /// Follows the scene: counts the time in the background, locks on the way back, and
    /// prompts once when the cover comes up.
    func scene(_ phase: ScenePhase) {
        switch phase {
        case .background:
            if backgroundSince == nil { backgroundSince = .now }
            // Back on screen still locked, the prompt comes up again.
            if locked { prompted = false }
            prompting = false
            inactive = true
        case .inactive:
            lockIfDue()
            inactive = true
        case .active:
            lockIfDue()
            prompting = false
            inactive = false
            #if DEBUG
            // -lockPrompt NO: the cover without the system prompt over it, for screenshots.
            if UserDefaults.standard.string(forKey: "lockPrompt") == "NO" { prompted = true }
            #endif
            if locked && !prompted {
                prompted = true
                Task { await unlock() }
            }
        @unknown default:
            break
        }
    }

    /// The system prompt; the cover goes once it succeeds.
    func unlock() async {
        guard locked, !authenticating else { return }
        switch await authenticate(String(localized: "Unlock lichen")) {
        case .success:
            locked = false
        case .noPasscode:
            // The device has no passcode any more: nothing can unlock lichen, and anyone holding
            // the phone could open it anyway. The lock goes rather than locking the owner out.
            enabled = false
            locked = false
        case .failed:
            break
        }
    }

    /// Turning it on takes one successful authentication first, so a method that cannot be
    /// used never locks the owner out. Turning it off takes none: lichen is open already.
    func setEnabled(_ on: Bool) async {
        guard on else {
            enabled = false
            return
        }
        guard !enabled, !authenticating else { return }
        switch await authenticate(String(localized: "Turn on the app lock")) {
        case .success:
            backgroundSince = nil
            enabled = true
        case .noPasscode:
            problem = String(localized: "Set a passcode on this iPhone first: lichen unlocks with it, or with Face ID or Touch ID.")
        case .failed:
            break
        }
    }

    // MARK: private

    private enum Outcome { case success, noPasscode, failed }

    private func authenticate(_ reason: String) async -> Outcome {
        let context = LAContext()
        var error: NSError?
        guard context.canEvaluatePolicy(.deviceOwnerAuthentication, error: &error) else {
            return (error as? LAError)?.code == .passcodeNotSet ? .noPasscode : .failed
        }
        authenticating = true
        prompting = true
        defer {
            authenticating = false
            // Still inactive: the prompt is going away, the scene is active again in a moment.
            if !inactive { prompting = false }
        }
        do {
            return try await context.evaluatePolicy(.deviceOwnerAuthentication, localizedReason: reason) ? .success : .failed
        } catch let e as LAError where e.code == .passcodeNotSet {
            return .noPasscode
        } catch {
            return .failed
        }
    }

    private func lockIfDue() {
        guard let since = backgroundSince else { return }
        backgroundSince = nil
        if enabled, !locked, since.duration(to: .now) >= .seconds(delay) {
            prompted = false
            locked = true
        }
    }

    /// Shows or hides the cover window. While it is up, VoiceOver reads nothing of the app's
    /// window behind it (chats, sheets, the copy of the cover).
    private func update() {
        defer { appWindow?.accessibilityElementsHidden = window?.isHidden == false }
        if covered {
            guard let window = window ?? makeWindow() else { return }
            if window.isHidden { window.isHidden = false }
            if locked, !window.isKeyWindow {
                // The keyboard goes with what it was typing into.
                appWindow?.endEditing(true)
                window.makeKey()
            }
        } else if let window, !window.isHidden {
            window.isHidden = true
            appWindow?.makeKey()
        }
    }

    private func makeWindow() -> UIWindow? {
        guard let cover,
              let scene = UIApplication.shared.connectedScenes.compactMap({ $0 as? UIWindowScene }).first
        else { return nil }
        appWindow = scene.windows.first { $0.windowLevel == .normal }
        let controller = UIHostingController(rootView: cover())
        controller.view.backgroundColor = .clear
        let window = UIWindow(windowScene: scene)
        window.windowLevel = .alert + 1
        window.rootViewController = controller
        window.accessibilityViewIsModal = true
        self.window = window
        return window
    }
}

/// What hides lichen while it is locked, or in the app switcher: the mark, an Unlock button,
/// and a line when an SOS came in meanwhile (its alert shows once unlocked).
struct LockCover: View {
    @Environment(AppModel.self) private var model
    @AppStorage("appearance") private var appearance = Appearance.system

    var body: some View {
        let lock = model.lock
        VStack(spacing: 0) {
            Spacer()
            VStack(spacing: 20) {
                ChirpMark()
                    .frame(width: 78, height: 78)
                    .frame(width: 120, height: 120)
                    .background(Theme.s1, in: RoundedRectangle(cornerRadius: 27))
                    .overlay(RoundedRectangle(cornerRadius: 27).stroke(Theme.line))
                Text(verbatim: "lichen")
                    .font(Theme.wordmark(48))
                    .tracking(-1.5)
                    .foregroundStyle(Theme.tx)
            }
            Spacer()
            VStack(spacing: 16) {
                if model.sosAlert != nil {
                    Label("SOS received: unlock to see it", systemImage: "exclamationmark.triangle.fill")
                        .font(.system(.callout, weight: .semibold))
                        .foregroundStyle(Theme.sos)
                        .multilineTextAlignment(.center)
                }
                Button("Unlock") { Task { await lock.unlock() } }
                    .buttonStyle(.primary)
                    .disabled(lock.authenticating)
            }
            // Only once locked: in the app switcher the cover just hides the app.
            .opacity(lock.locked ? 1 : 0)
            .allowsHitTesting(lock.locked)
            .accessibilityHidden(!lock.locked)
        }
        .padding(.horizontal, 24)
        .padding(.bottom, 12)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Theme.bg.ignoresSafeArea())
        .tint(Theme.tx)
        .preferredColorScheme(appearance.scheme)
    }
}

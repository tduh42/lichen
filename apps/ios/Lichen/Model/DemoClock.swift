#if DEBUG
import UIKit

/// The demo's clock for the UI tests (`-demoClock YES`): the core's time, in seconds since the
/// demo started, as the accessibility value of an element `demoClock`.
///
/// With `-demoSpeed`, the app's clock counts at most 0.25 s of the wall clock per reading
/// (`AppModel.now()`): while the main thread is busy (a test's accessibility queries, a heavy
/// screen such as the map), the demo falls behind the wall clock. A test that waits for one of
/// the demo's events waits on this clock instead (`LichenUITest.waitDemo`, F-demo-timing).
///
/// The element lives in a window of its own, one point in the top left corner, that takes no
/// touch and never becomes key: it stays readable over sheets, full-screen alerts and the lock
/// cover, and changes nothing the user sees.
@MainActor
final class DemoClock {
    private var window: UIWindow?
    private var shown: UInt64?

    /// Shows `ms` of the core's clock, to a tenth of a second.
    func show(_ ms: UInt64) {
        let tenths = ms / 100
        guard tenths != shown else { return }
        if window == nil { window = makeWindow() }
        guard let view = window?.rootViewController?.view else { return }
        shown = tenths
        view.accessibilityValue = "\(tenths / 10).\(tenths % 10)"
    }

    private func makeWindow() -> UIWindow? {
        guard let scene = UIApplication.shared.connectedScenes.compactMap({ $0 as? UIWindowScene }).first
        else { return nil }
        let controller = UIViewController()
        controller.view.backgroundColor = .clear
        controller.view.isAccessibilityElement = true
        controller.view.accessibilityIdentifier = "demoClock"
        let window = UIWindow(windowScene: scene)
        window.frame = CGRect(x: 0, y: 0, width: 1, height: 1)
        window.windowLevel = .alert + 2
        window.rootViewController = controller
        window.isUserInteractionEnabled = false
        window.isHidden = false
        return window
    }
}
#endif

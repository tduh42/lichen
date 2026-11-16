import XCTest

/// The radio's settings on the demo network (`-screen radio`, ADR-0032): what the radio reports,
/// and a change that asks first, then comes back from the radio.
final class RadioSettingsTests: XCTestCase {
    override func setUp() {
        continueAfterFailure = false
    }

    private func launch() -> XCUIApplication {
        let app = XCUIApplication()
        app.launchArguments = ["-demo", "YES", "-onboarded", "YES", "-tab", "settings", "-screen", "radio"]
        app.launch()
        let allow = XCUIApplication(bundleIdentifier: "com.apple.springboard").buttons["Allow"]
        if allow.waitForExistence(timeout: 5) { allow.tap() }
        return app
    }

    /// Scrolls until `element` can be tapped.
    private func reveal(_ element: XCUIElement, in app: XCUIApplication) {
        for _ in 0..<12 where !(element.exists && element.isHittable) {
            app.swipeUp()
        }
        XCTAssertTrue(element.isHittable, "\(element) not reachable")
    }

    /// Flips a switch whose label is hidden (the row shows its title).
    private func flip(_ toggle: XCUIElement) {
        let before = toggle.value as? String
        toggle.tap()
        if toggle.value as? String == before {
            toggle.coordinate(withNormalizedOffset: CGVector(dx: 0.9, dy: 0.5)).tap()
        }
    }

    func testTheRadioReportsItsSettingsAndAChangeComesBack() {
        let app = launch()
        // SELF_INFO, read at connection.
        let now = app.staticTexts.containing(NSPredicate(format: "label BEGINSWITH '869.618 MHz'")).firstMatch
        XCTAssertTrue(now.waitForExistence(timeout: 30), "the radio's parameters are not shown")
        // The demo's board has a GPS, off.
        let gps = app.switches["The radio's GPS"]
        reveal(gps, in: app)
        XCTAssertEqual(gps.value as? String, "0")
        flip(gps)
        XCTAssertTrue(app.staticTexts["A fix every"].waitForExistence(timeout: 10), "the GPS did not come back on")
        // A position in adverts asks first.
        let position = app.switches["Position in adverts"]
        reveal(position, in: app)
        flip(position)
        let confirm = app.alerts["Show your position to the whole mesh?"]
        XCTAssertTrue(confirm.waitForExistence(timeout: 5), "no confirmation")
        confirm.buttons["Show it"].tap()
        let on = app.staticTexts["Every advert shows the radio's position to the whole mesh, unencrypted."]
        XCTAssertTrue(on.waitForExistence(timeout: 10), "the radio did not report the new policy")
        reveal(app.staticTexts["Restart the radio"], in: app)
    }
}

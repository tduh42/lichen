import XCTest

/// A long conversation on the demo network (`-fill 5000`: 5,000 older messages with Camille):
/// how long it takes to open, and how smoothly it scrolls (docs/measurements.md). Measurements,
/// skipped unless the runner has `LICHEN_MEASURE` set:
///
///     TEST_RUNNER_LICHEN_MEASURE=1 xcodebuild test … -only-testing:LichenUITests/LongChatTests
final class LongChatTests: XCTestCase {
    override func setUpWithError() throws {
        try XCTSkipUnless(ProcessInfo.processInfo.environment["LICHEN_MEASURE"] != nil, "measurements only")
        continueAfterFailure = false
    }

    private func launch(_ arguments: [String]) -> XCUIApplication {
        let app = XCUIApplication()
        app.launchArguments = ["-demo", "YES", "-onboarded", "YES", "-fill", "5000"] + arguments
        app.launch()
        let allow = XCUIApplication(bundleIdentifier: "com.apple.springboard").buttons["Allow"]
        if allow.waitForExistence(timeout: 5) { allow.tap() }
        return app
    }

    private func camille(_ app: XCUIApplication) -> XCUIElement {
        app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Camille'")).firstMatch
    }

    /// Opening the chat on its last page (100 messages), five times: the push until the app is
    /// idle again (XCTest waits for it after a tap), and back.
    func testOpen() {
        let app = launch([])
        let row = camille(app)
        XCTAssertTrue(row.waitForExistence(timeout: 120), "no Camille")
        let options = XCTMeasureOptions()
        options.iterationCount = 5
        measure(metrics: [XCTClockMetric(), XCTOSSignpostMetric.navigationTransitionMetric], options: options) {
            row.tap()
            XCTAssertTrue(app.descendants(matching: .any)["composer"].waitForExistence(timeout: 30))
            app.navigationBars.buttons.element(boundBy: 0).tap()
            XCTAssertTrue(row.waitForExistence(timeout: 30))
        }
    }

    /// A search result 3,000 messages back (`-focus`): the chat opens on it, every message in
    /// between loaded. Then flicks through them.
    func testJumpBackAndScroll() {
        let app = launch(["-focus", "Earlier message 1999"])
        let row = camille(app)
        XCTAssertTrue(row.waitForExistence(timeout: 120), "no Camille")
        let start = Date()
        row.tap()
        XCTAssertTrue(app.staticTexts["Earlier message 1999"].waitForExistence(timeout: 120), "not on the result")
        let opened = Date().timeIntervalSince(start)
        print(String(format: "M5 measure: jump back 3,000 messages: %.2f s", opened))
        XCTAssertTrue(app.staticTexts["Earlier message 1999"].isHittable, "the result is off screen")
        let list = app.scrollViews.firstMatch
        let options = XCTMeasureOptions()
        options.iterationCount = 5
        // The simulator reports no hitches: the app's CPU time and the time until it is idle
        // again stand for smoothness.
        measure(metrics: [XCTOSSignpostMetric.scrollDecelerationMetric, XCTClockMetric(), XCTCPUMetric(application: app)],
                options: options) {
            list.swipeUp(velocity: .fast)
        }
    }

    /// The last page, flicked up and down.
    func testScroll() {
        let app = launch(["-open", "Camille"])
        XCTAssertTrue(app.descendants(matching: .any)["composer"].waitForExistence(timeout: 120))
        sleep(3)
        let list = app.scrollViews.firstMatch
        let options = XCTMeasureOptions()
        options.iterationCount = 6
        var up = true
        measure(metrics: [XCTOSSignpostMetric.scrollDecelerationMetric, XCTClockMetric(), XCTCPUMetric(application: app)],
                options: options) {
            if up { list.swipeDown(velocity: .fast) } else { list.swipeUp(velocity: .fast) }
            up.toggle()
        }
    }
}

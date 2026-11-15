import XCTest

/// The chat screen on the demo network (`-demo YES`), driven as a user drives it: leaving a
/// conversation, and reopening a long one.
final class ChatNavigationTests: XCTestCase {
    override func setUp() {
        continueAfterFailure = false
    }

    private func launch(_ arguments: [String]) -> XCUIApplication {
        let app = XCUIApplication()
        app.launchArguments = ["-demo", "YES", "-onboarded", "YES"] + arguments
        app.launch()
        // The notification permission, asked once per install.
        let allow = XCUIApplication(bundleIdentifier: "com.apple.springboard").buttons["Allow"]
        if allow.waitForExistence(timeout: 5) { allow.tap() }
        return app
    }

    private func inChat(_ app: XCUIApplication) -> XCUIElement {
        app.descendants(matching: .any)["composer"]
    }

    private func encrypted(_ app: XCUIApplication) -> XCUIElement {
        app.staticTexts.containing(NSPredicate(format: "label BEGINSWITH 'Encrypted session'")).firstMatch
    }

    /// A chat opened from elsewhere (as a notification opens it) closes with Back, and the
    /// conversation list stays (it used to open the same chat again).
    func testBackStaysOnTheList() {
        let app = launch(["-open", "Camille"])
        XCTAssertTrue(inChat(app).waitForExistence(timeout: 60), "the chat did not open")
        // Her lichen conversation, once her identity is known (before, `-open` shows her
        // MeshCore chat and switches once).
        XCTAssertTrue(encrypted(app).waitForExistence(timeout: 90), "no session with Camille")
        app.navigationBars.buttons.element(boundBy: 0).tap()
        sleep(5)
        XCTAssertFalse(inChat(app).exists, "Back opened a chat again")
        // Another tab, then Messages again: still the list.
        app.tabBars.buttons.element(boundBy: 1).tap()
        sleep(2)
        app.tabBars.buttons.element(boundBy: 0).tap()
        sleep(3)
        XCTAssertFalse(inChat(app).exists, "coming back to Messages opened a chat again")
        // A while later, messages arriving: still the list.
        sleep(20)
        XCTAssertFalse(inChat(app).exists, "a chat opened by itself")
    }

    /// Tapping a notification opens its chat. It used to abort the app: the delegate's
    /// completion ran off the main thread.
    func testTappingANotificationOpensItsChat() {
        let app = launch([])
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        // Camille's first message comes while the list is on screen: a banner.
        let banner = springboard.descendants(matching: .any)
            .matching(NSPredicate(format: "label CONTAINS 'Camille' AND label CONTAINS 'refuge'")).firstMatch
        XCTAssertTrue(banner.waitForExistence(timeout: 120), "no banner")
        banner.tap()
        XCTAssertTrue(inChat(app).waitForExistence(timeout: 10), "her chat did not open")
        sleep(2)
        XCTAssertEqual(app.state, .runningForeground, "the app is gone")
    }

    /// A row tapped twice while a long chat opens (it takes a moment) opens it once: Back
    /// returns to the list, not to the same chat.
    func testATappedTwiceRowOpensOnce() {
        let app = launch(["-fill", "250"])
        let row = app.buttons.containing(NSPredicate(format: "label CONTAINS 'Camille'")).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 90), "no Camille")
        row.doubleTap()
        XCTAssertTrue(inChat(app).waitForExistence(timeout: 10))
        sleep(2)
        app.navigationBars.buttons.element(boundBy: 0).tap()
        sleep(3)
        XCTAssertFalse(inChat(app).exists, "Back landed on the same chat")
    }

    /// Every conversation of the list opens the one tapped, and Back returns to the list, with
    /// messages arriving meanwhile.
    func testEveryConversationOpensAndCloses() {
        let app = launch([])
        let names = ["Camille", "Rando Vercors", "Marc", "Public"]
        // The demo's group comes last, once its keys are out.
        let group = app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Rando Vercors'")).firstMatch
        XCTAssertTrue(group.waitForExistence(timeout: 240), "the list is not complete")
        for round in 0..<2 {
            for name in names {
                // The row whose label starts with the name (a preview may mention someone else).
                let row = app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", name)).firstMatch
                XCTAssertTrue(row.waitForExistence(timeout: 30),
                              "no row \(name) in \(app.buttons.allElementsBoundByIndex.map(\.label))")
                let tapped = row.label
                row.tap()
                XCTAssertTrue(inChat(app).waitForExistence(timeout: 10), "\(name) did not open")
                let bar = app.navigationBars.firstMatch
                let title = bar.descendants(matching: .any)
                    .matching(NSPredicate(format: "label BEGINSWITH %@", name)).firstMatch
                let shown = bar.descendants(matching: .any).allElementsBoundByIndex.map(\.label)
                XCTAssertTrue(title.waitForExistence(timeout: 5),
                              "round \(round): tapped “\(tapped)”, the bar shows \(shown)")
                sleep(3)
                app.navigationBars.buttons.element(boundBy: 0).tap()
                sleep(2)
                XCTAssertFalse(inChat(app).exists, "round \(round): Back from \(name) did not reach the list")
            }
        }
    }

    /// A long chat with a photo, opened again once everything is read, shows its last message.
    func testALongChatOpensAtTheEnd() {
        let app = launch(["-open", "Camille", "-fill", "250"])
        XCTAssertTrue(inChat(app).waitForExistence(timeout: 60))
        XCTAssertTrue(encrypted(app).waitForExistence(timeout: 90), "no session with Camille")
        // Her photo comes about two minutes in.
        let photo = app.staticTexts["The view from the refuge tonight:"]
        XCTAssertTrue(photo.waitForExistence(timeout: 240), "no photo")
        sleep(30)
        app.navigationBars.buttons.element(boundBy: 0).tap()
        let row = app.buttons.containing(NSPredicate(format: "label CONTAINS 'Camille'")).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 10))
        row.tap()
        XCTAssertTrue(inChat(app).waitForExistence(timeout: 10))
        sleep(4)
        let end = app.descendants(matching: .any)["chat.end"]
        let composer = inChat(app)
        XCTAssertTrue(end.exists)
        let window = app.windows.firstMatch.frame
        XCTAssertGreaterThan(end.frame.minY, window.minY, "end above the screen")
        XCTAssertLessThanOrEqual(end.frame.minY, composer.frame.minY, "end below the screen: \(end.frame) vs \(composer.frame)")
    }
}

import XCTest

/// Messages lists every conversation that has a message, whatever the radio's contact list says
/// (protocol.md §15.8): the refuge's room server, and a person the radio files as a repeater.
/// `TEST_RUNNER_LICHEN_LANG=fr` runs them in French. Each keeps a screenshot of the list and of
/// the chat it opens.
final class ConversationListTests: XCTestCase {
    private var french: Bool { ProcessInfo.processInfo.environment["LICHEN_LANG"] == "fr" }

    override func setUp() {
        continueAfterFailure = false
    }

    private func launch(_ arguments: [String]) -> XCUIApplication {
        let app = XCUIApplication()
        let language = french ? ["-AppleLanguages", "(fr)", "-AppleLocale", "fr_FR"] : ["-AppleLanguages", "(en)", "-AppleLocale", "en_GB"]
        app.launchArguments = ["-demo", "YES", "-onboarded", "YES"] + language + arguments
        app.launch()
        // The notification permission, asked once per install.
        let allow = XCUIApplication(bundleIdentifier: "com.apple.springboard").buttons
            .matching(NSPredicate(format: "label IN %@", ["Allow", "Autoriser"])).firstMatch
        if allow.waitForExistence(timeout: 5) { allow.tap() }
        return app
    }

    private func inChat(_ app: XCUIApplication) -> XCUIElement {
        app.descendants(matching: .any)["composer"]
    }

    private func keep(_ app: XCUIApplication, _ name: String) {
        let shot = XCTAttachment(screenshot: app.screenshot())
        shot.name = (french ? "fr-" : "en-") + name
        shot.lifetime = .keepAlways
        add(shot)
    }

    /// The row whose label starts with `name`, with its badge, then the chat it opens, named
    /// in the bar.
    private func opens(_ app: XCUIApplication, _ name: String, badge: String, shot: String) {
        let row = app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", name)).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 150),
                      "no row \(name) in \(app.buttons.allElementsBoundByIndex.map(\.label))")
        XCTAssertTrue(row.label.contains(badge), "row “\(row.label)” has no “\(badge)”")
        sleep(2)
        keep(app, shot + "-messages")
        row.tap()
        XCTAssertTrue(inChat(app).waitForExistence(timeout: 10), "\(name) did not open")
        let title = app.navigationBars.firstMatch.descendants(matching: .any)
            .matching(NSPredicate(format: "label BEGINSWITH %@", name)).firstMatch
        XCTAssertTrue(title.waitForExistence(timeout: 5), "the bar does not name \(name)")
        sleep(2)
        keep(app, shot + "-chat")
        app.navigationBars.buttons.element(boundBy: 0).tap()
        sleep(2)
        XCTAssertFalse(inChat(app).exists, "Back from \(name) did not reach the list")
    }

    /// Logged in to the board (`-roomLogin`), its posts make a row with the room's badge, and
    /// the row opens the board.
    func testTheRoomHasItsRow() {
        let app = launch(["-roomLogin", "hello"])
        opens(app, "Refuge board", badge: french ? "Salon MeshCore" : "MeshCore room", shot: "room")
    }

    /// The radio files Marc as a repeater (`-marcRepeater`): he is no longer one of the people,
    /// and his MeshCore chat keeps its row, under his name.
    func testAPersonFiledAsARepeaterKeepsTheirRow() {
        let app = launch(["-marcRepeater", "YES"])
        opens(app, "Marc", badge: "MeshCore", shot: "marc-repeater")
    }
}

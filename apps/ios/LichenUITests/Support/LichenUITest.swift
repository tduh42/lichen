import XCTest

/// The base of lichen's UI tests: the app launched as a test needs it (the demo network by
/// default), in the language and appearance the run asks for, and ways to wait for what a
/// user waits for. Test methods are named after the scenarios of docs/testing.md.
///
/// The run's environment (`xcodebuild` passes `TEST_RUNNER_<NAME>` to the runner as `<NAME>`;
/// `scripts/ui-tests-ios.sh` sets them):
/// - `LICHEN_LANG=fr`: the app in French, else English;
/// - `LICHEN_DARK=1`: the app in dark mode, else light.
///
/// Every launch brings the demo's script 20 s sooner (`-demoSkip 20`) and runs it 4 times
/// faster than the wall clock (`-demoSpeed 4`): Camille's first message comes about 5 s in.
class LichenUITest: XCTestCase {
    private(set) var app = XCUIApplication()

    static var french: Bool { ProcessInfo.processInfo.environment["LICHEN_LANG"] == "fr" }
    static var dark: Bool { ProcessInfo.processInfo.environment["LICHEN_DARK"] == "1" }
    var french: Bool { Self.french }

    /// How long the demo's first events take, with `-demoSkip 20`, and some margin.
    static let camilleTimeout: TimeInterval = 60

    override func setUp() {
        super.setUp()
        continueAfterFailure = false
        // The notification permission, asked once per install, whenever it shows.
        addUIInterruptionMonitor(withDescription: "permission") { alert in
            let allow = alert.buttons.matching(NSPredicate(format: "label IN %@", ["Allow", "Autoriser", "OK"])).firstMatch
            guard allow.exists else { return false }
            allow.tap()
            return true
        }
    }

    // MARK: Launching

    /// Launches the app: on the demo network unless `demo` is false, past the welcome screens
    /// unless `onboarded` is false, with `arguments` (CLAUDE.md lists them).
    @discardableResult
    func launch(_ arguments: [String] = [], demo: Bool = true, onboarded: Bool = true, skip: Int = 20,
                speed: Int = 4, fresh: Bool = true, banners: Bool = false, dark: Bool? = nil) -> XCUIApplication {
        app = XCUIApplication()
        var args = arguments
        func setting(_ key: String, _ value: String) {
            if !args.contains("-" + key) { args += ["-" + key, value] }
        }
        if demo {
            setting("demo", "YES")
            if skip > 0 { setting("demoSkip", String(skip)) }
            // The demo 4 times faster than the wall clock (D-8): Camille's first message about
            // 5 s in.
            if speed > 1 { setting("demoSpeed", String(speed)) }
        }
        // Nothing an earlier test left (drafts, settings) changes this one, in the demo or not;
        // a relaunch keeps what this one made.
        if fresh { setting("freshInstall", "YES") }
        if onboarded { setting("onboarded", "YES") }
        // No banner over the app unless the test looks at them: taps near the top wait behind
        // one for up to 15 s (a minute of the demo's time).
        if !banners { setting("noBanners", "YES") }
        args += french ? ["-AppleLanguages", "(fr)", "-AppleLocale", "fr_FR"]
                       : ["-AppleLanguages", "(en)", "-AppleLocale", "en_GB"]
        // What earlier runs left in the settings must not change this one.
        setting("appearance", (dark ?? Self.dark) ? "dark" : "light")
        setting("lichen.radioDetails", "NO")
        app.launchArguments = args
        app.launch()
        // Asked once per install: looked for at the run's first launch only (the interruption
        // monitor answers it when it shows later, after the welcome screens).
        if !Self.permissionLooked {
            Self.permissionLooked = true
            answerNotifications()
        }
        return app
    }

    private static var permissionLooked = false

    /// The app again, as the user opens it after it was closed: same data, same language.
    func relaunch(_ arguments: [String] = [], demo: Bool = false, onboarded: Bool = false) {
        app.terminate()
        launch(arguments, demo: demo, onboarded: onboarded || demo, skip: demo ? 20 : 0, fresh: false)
    }

    /// Taps Allow on the notification permission when it shows (once per install).
    func answerNotifications(timeout: TimeInterval = 3) {
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        let allow = springboard.buttons.matching(NSPredicate(format: "label IN %@", ["Allow", "Autoriser"])).firstMatch
        if allow.waitForExistence(timeout: timeout) { allow.tap() }
    }

    // MARK: Words

    /// The app's text for `key` (its String Catalog, copied into this bundle) in the run's
    /// language, plurals included. Format keys take `%@`, `%lld` or `%u` arguments, as the app
    /// passes them.
    func tr(_ key: String, _ args: CVarArg...) -> String {
        let bundle = french ? Self.frenchBundle : Self.englishBundle
        let format = bundle?.localizedString(forKey: key, value: key, table: "Localizable") ?? key
        return args.isEmpty ? format : String(format: format, locale: Locale(identifier: french ? "fr_FR" : "en_GB"), arguments: args)
    }

    private static let frenchBundle = bundle("fr")
    private static let englishBundle = bundle("en")

    private static func bundle(_ language: String) -> Bundle? {
        Bundle(for: LichenUITest.self).path(forResource: language, ofType: "lproj").flatMap(Bundle.init(path:))
    }

    // MARK: Finding

    /// Any element whose label is `label`.
    func element(_ label: String) -> XCUIElement {
        app.descendants(matching: .any).matching(NSPredicate(format: "label == %@", label)).firstMatch
    }

    /// Any element whose label contains `text`.
    func containing(_ text: String) -> XCUIElement {
        app.descendants(matching: .any).matching(NSPredicate(format: "label CONTAINS %@", text)).firstMatch
    }

    /// How many elements have a label containing `text` (a message shown twice counts two).
    func count(containing text: String, type: XCUIElement.ElementType = .any) -> Int {
        app.descendants(matching: type).matching(NSPredicate(format: "label CONTAINS %@", text)).count
    }

    func button(_ label: String) -> XCUIElement {
        app.buttons.matching(NSPredicate(format: "label == %@", label)).firstMatch
    }

    /// A settings-like row (a title, then its subtitle in the same label): the button whose
    /// label starts with `title`.
    func cardButton(_ title: String) -> XCUIElement {
        app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", title)).firstMatch
    }

    /// A conversation's row in Messages: the button whose label starts with its name.
    func row(_ name: String) -> XCUIElement {
        app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", name)).firstMatch
    }

    var composer: XCUIElement { app.descendants(matching: .any)["composer"] }

    /// The navigation bar's back button.
    func back() {
        app.navigationBars.buttons.element(boundBy: 0).tap()
    }

    enum Tab: Int { case messages, map, locate, settings }

    func tab(_ tab: Tab) {
        app.tabBars.buttons.element(boundBy: tab.rawValue).tap()
    }

    // MARK: Waiting

    /// Waits for `element`, or fails with `message`.
    @discardableResult
    func wait(_ element: XCUIElement, _ timeout: TimeInterval = 10, _ message: @autoclosure () -> String = "",
              file: StaticString = #filePath, line: UInt = #line) -> XCUIElement {
        if !element.waitForExistence(timeout: timeout) {
            let text = message()
            attachHierarchy()
            XCTFail(text.isEmpty ? "no \(element) after \(Int(timeout)) s" : text, file: file, line: line)
        }
        return element
    }

    /// The app's accessibility tree, kept with a failure.
    func attachHierarchy() {
        let tree = XCTAttachment(string: app.debugDescription)
        tree.name = "hierarchy"
        tree.lifetime = .keepAlways
        add(tree)
    }

    /// How many messages are `text`, whole: a bubble's label is the text, then the time
    /// (", 13:05"); its text is also exposed alone, and is not counted.
    func bubbles(_ text: String) -> Int {
        app.scrollViews.descendants(matching: .any).matching(NSPredicate(format: "label BEGINSWITH %@", text + ", ")).count
    }

    /// Waits for an element whose label contains `text`.
    @discardableResult
    func waitText(_ text: String, _ timeout: TimeInterval = 10, file: StaticString = #filePath, line: UInt = #line) -> XCUIElement {
        wait(containing(text), timeout, "no “\(text)” after \(Int(timeout)) s", file: file, line: line)
    }

    /// Waits until `element` is gone.
    func waitGone(_ element: XCUIElement, _ timeout: TimeInterval = 10, _ message: @autoclosure () -> String = "",
                  file: StaticString = #filePath, line: UInt = #line) {
        let gone = XCTNSPredicateExpectation(predicate: NSPredicate(format: "exists == false"), object: element)
        if XCTWaiter().wait(for: [gone], timeout: timeout) != .completed {
            let text = message()
            XCTFail(text.isEmpty ? "\(element) still there after \(Int(timeout)) s" : text, file: file, line: line)
        }
    }

    /// Waits until `element`'s label contains `text`. Polls, so that an element that goes
    /// meanwhile (a message that disappears) fails the wait instead of stalling it.
    func waitLabel(_ element: XCUIElement, contains text: String, _ timeout: TimeInterval = 10,
                   file: StaticString = #filePath, line: UInt = #line) {
        let end = Date().addingTimeInterval(timeout)
        var last = "(gone)"
        repeat {
            if element.exists {
                last = element.label
                if last.contains(text) { return }
            }
            pause(0.5)
        } while Date() < end
        attachHierarchy()
        XCTFail("“\(last)” has no “\(text)” after \(Int(timeout)) s", file: file, line: line)
    }

    /// Does nothing for `seconds`, the app running: for what must not happen meanwhile.
    func pause(_ seconds: TimeInterval) {
        _ = XCTWaiter().wait(for: [XCTestExpectation(description: "pause")], timeout: seconds)
    }

    // MARK: Doing

    /// Opens the conversation `name` from Messages.
    func openChat(_ name: String, _ timeout: TimeInterval = camilleTimeout, file: StaticString = #filePath, line: UInt = #line) {
        let r = wait(row(name), timeout, "no row \(name) in \(app.buttons.allElementsBoundByIndex.map(\.label))", file: file, line: line)
        r.tap()
        wait(composer, 10, "\(name) did not open", file: file, line: line)
    }

    /// Camille's lichen chat, once her identity is known: the key button in the bar.
    var keyButton: XCUIElement {
        app.navigationBars.buttons.matching(NSPredicate(format: "label IN %@",
            [tr("Verify their key"), tr("Key verified"), tr("Their key changed: compare")])).firstMatch
    }

    /// Camille's lichen chat, once `-open Camille` opened it: her key button in the bar. If the
    /// debug switch from her MeshCore chat to her lichen one was missed (it races her first
    /// message), her row opens it, as a user would.
    func openedCamille(file: StaticString = #filePath, line: UInt = #line) {
        let bar = app.navigationBars.firstMatch.descendants(matching: .any)
            .matching(NSPredicate(format: "label BEGINSWITH 'Camille'")).firstMatch
        wait(bar, 30, "her chat did not open", file: file, line: line)
        if keyButton.waitForExistence(timeout: Self.camilleTimeout / 2) { return }
        back()
        let r = wait(row("Camille"), Self.camilleTimeout / 2, "no row for Camille", file: file, line: line)
        r.tap()
        wait(keyButton, 10, "no session with Camille", file: file, line: line)
    }

    /// A "MeshCore public key" row (F-1): its key, 64 hex digits, checked; a tap copies it
    /// ("Copied"). Returns the key.
    @discardableResult
    func checkMeshCoreKey(_ row: XCUIElement, file: StaticString = #filePath, line: UInt = #line) -> String {
        reveal(row, file: file, line: line)
        let hex = row.value as? String ?? ""
        XCTAssertNotNil(hex.range(of: "^[0-9a-f]{64}$", options: .regularExpression), "the key: \(hex)", file: file, line: line)
        row.tap()
        waitValue(row, tr("Copied"), 5, file: file, line: line)
        return hex
    }

    /// What the app pastes, read in its message search (the test runner may not read the
    /// pasteboard itself): Messages › Search, Paste, the field's text, Close.
    func pasted(file: StaticString = #filePath, line: UInt = #line) -> String {
        tab(.messages)
        button(tr("Search messages")).tap()
        let field = wait(app.textFields[tr("Search messages")], 5, "no search field", file: file, line: line)
        field.press(forDuration: 1.2)
        wait(app.menuItems.matching(NSPredicate(format: "label IN %@", ["Paste", "Coller"])).firstMatch, 5, "no Paste",
             file: file, line: line).tap()
        let text = field.value as? String ?? ""
        button(tr("Close")).tap()
        return text
    }

    /// Waits until `element`'s value is `value`.
    func waitValue(_ element: XCUIElement, _ value: String, _ timeout: TimeInterval,
                   file: StaticString = #filePath, line: UInt = #line) {
        let deadline = Date().addingTimeInterval(timeout)
        while (element.value as? String) != value && Date() < deadline { pause(0.3) }
        XCTAssertEqual(element.value as? String, value, file: file, line: line)
    }

    /// The "MeshCore public key" row on screen.
    var meshcoreKey: XCUIElement { app.descendants(matching: .any)["meshcoreKey"] }

    /// Types `text` in the composer and sends it.
    func send(_ text: String, file: StaticString = #filePath, line: UInt = #line) {
        composer.tap()
        composer.typeText(text)
        let send = button(tr("Send"))
        wait(send, 5, "no Send button", file: file, line: line)
        send.tap()
    }

    /// The message that starts with `text` (a text bubble reads as one element: its text, then
    /// its time and status); not a quote of it, nor an answer quoting it.
    func message(_ text: String) -> XCUIElement {
        app.scrollViews.descendants(matching: .any).matching(NSPredicate(format: "label BEGINSWITH %@", text)).firstMatch
    }

    /// A message's text alone, `text` whole: what to long-press (the bubble's centre may be on
    /// a reaction chip or a quote).
    func textOf(_ text: String) -> XCUIElement {
        app.scrollViews.staticTexts.matching(NSPredicate(format: "label == %@", text)).firstMatch
    }

    /// The bubble holding `text` with `status` (a reply's label starts with its quote).
    func bubble(_ text: String, with status: String) -> XCUIElement {
        app.scrollViews.descendants(matching: .any)
            .matching(NSPredicate(format: "label CONTAINS %@ AND label CONTAINS %@", text, status)).firstMatch
    }

    /// Long-presses a message and taps `action` in its menu. With the keyboard up, the first
    /// press may only lower it: pressed again then; a tap the menu ignores (it is still open:
    /// its Info item is there) is made again on the item's centre.
    func menu(_ element: XCUIElement, _ action: String, file: StaticString = #filePath, line: UInt = #line) {
        let item = button(action)
        for _ in 0..<2 where !item.exists {
            element.press(forDuration: 1.2)
            _ = item.waitForExistence(timeout: 3)
        }
        wait(item, 2, "no “\(action)” in the menu", file: file, line: line)
        let frame = item.frame
        item.tap()
        let open = button(tr("Info"))
        if action != tr("Info"), open.waitForExistence(timeout: 1.5), open.isHittable {
            app.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: frame.midX, dy: frame.midY)).tap()
        }
    }

    /// Scrolls `scroll` (the app by default) until `element` can be tapped.
    func reveal(_ element: XCUIElement, in scroll: XCUIElement? = nil, up: Bool = true,
                file: StaticString = #filePath, line: UInt = #line) {
        let target = scroll ?? app
        for _ in 0..<15 where !(element.exists && element.isHittable) {
            if up { target.swipeUp() } else { target.swipeDown() }
        }
        XCTAssertTrue(element.exists && element.isHittable, "\(element) not reachable", file: file, line: line)
    }

    /// Keeps a screenshot of the screen in the results, named for the run: "fr-dark-messages".
    func keep(_ name: String) {
        let shot = XCTAttachment(screenshot: app.screenshot())
        shot.name = (french ? "fr-" : "en-") + (Self.dark ? "dark-" : "") + name
        shot.lifetime = .keepAlways
        add(shot)
    }
}

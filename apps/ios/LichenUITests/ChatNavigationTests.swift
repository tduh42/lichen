import XCTest

/// The chat screen on the demo network, driven as a user drives it: leaving a conversation,
/// and reopening a long one (regressions found before docs/testing.md).
final class ChatNavigationTests: LichenUITest {
    private func inChat(_ app: XCUIApplication) -> XCUIElement {
        app.descendants(matching: .any)["composer"]
    }

    /// Her lichen conversation, with a session: the key button in the bar (the session line at
    /// the top of the list is not built while a long chat shows its end).
    private func encrypted(_ app: XCUIApplication) -> XCUIElement { keyButton }

    /// A chat opened from elsewhere (as a notification opens it) closes with Back, and the
    /// conversation list stays (it used to open the same chat again).
    func testBackStaysOnTheList() {
        launch(["-open", "Camille"])
        XCTAssertTrue(inChat(app).waitForExistence(timeout: 60), "the chat did not open")
        // Her lichen conversation, once her identity is known (before, `-open` shows her
        // MeshCore chat and switches once).
        XCTAssertTrue(encrypted(app).waitForExistence(timeout: 90), "no session with Camille")
        app.navigationBars.buttons.element(boundBy: 0).tap()
        sleep(5)
        XCTAssertFalse(inChat(app).exists, "Back opened a chat again")
        // Another tab, then Messages again: still the list.
        tab(.map)
        sleep(2)
        tab(.messages)
        sleep(3)
        XCTAssertFalse(inChat(app).exists, "coming back to Messages opened a chat again")
        // A while later (a minute and more of the demo's time at speed 4), messages arriving:
        // still the list.
        sleep(6)
        XCTAssertFalse(inChat(app).exists, "a chat opened by itself")
    }

    /// Tapping a notification opens its chat. It used to abort the app: the delegate's
    /// completion ran off the main thread.
    func testTappingANotificationOpensItsChat() {
        launch([], banners: true)
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
        launch(["-fill", "250"])
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
        launch([])
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
                var tapped = settled(row).label
                row.tap()
                XCTAssertTrue(inChat(app).waitForExistence(timeout: 10), "\(name) did not open")
                let bar = app.navigationBars.firstMatch
                let title = bar.descendants(matching: .any)
                    .matching(NSPredicate(format: "label BEGINSWITH %@", name)).firstMatch
                // A message arriving reorders the list under the tap now and then (the demo is
                // busy, the runner slow): another chat opens. Back, and once more.
                if !title.waitForExistence(timeout: 3) {
                    app.navigationBars.buttons.element(boundBy: 0).tap()
                    XCTAssertTrue(row.waitForExistence(timeout: 10))
                    tapped = settled(row).label
                    row.tap()
                    XCTAssertTrue(inChat(app).waitForExistence(timeout: 10), "\(name) did not open")
                }
                let shown = bar.descendants(matching: .any).allElementsBoundByIndex.map(\.label)
                XCTAssertTrue(title.waitForExistence(timeout: 5),
                              "round \(round): tapped “\(tapped)”, the bar shows \(shown)")
                sleep(1)
                app.navigationBars.buttons.element(boundBy: 0).tap()
                sleep(1)
                XCTAssertFalse(inChat(app).exists, "round \(round): Back from \(name) did not reach the list")
            }
        }
    }

    /// A long chat opened at its end stays at its end while its rows take their real heights,
    /// however late (`-rowsGrowAfter 3`: every row 24 pt taller 3 s after it shows, after the
    /// opening's last correction). It used to stop short of the end now and then (a photo laid
    /// out late, about one run in nine).
    func testALongChatStaysAtItsEndWhileItsRowsGrow() {
        launch(["-open", "Camille", "-fill", "250", "-quietDemo", "YES", "-rowsGrowAfter", "3", "-viewShrinksAfter", "4"])
        XCTAssertTrue(inChat(app).waitForExistence(timeout: 60))
        XCTAssertTrue(encrypted(app).waitForExistence(timeout: 90), "no session with Camille")
        // Everything read: reopened, the chat opens at its end.
        pause(3)
        app.navigationBars.buttons.element(boundBy: 0).tap()
        let row = app.buttons.containing(NSPredicate(format: "label CONTAINS 'Camille'")).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 10))
        row.tap()
        XCTAssertTrue(inChat(app).waitForExistence(timeout: 10))
        // Past the rows' growth.
        pause(6)
        let end = app.descendants(matching: .any)["chat.end"]
        XCTAssertTrue(end.exists, "the end is not laid out")
        XCTAssertLessThanOrEqual(end.frame.minY, inChat(app).frame.minY, "the end went below the screen as the rows grew: \(end.frame) vs \(inChat(app).frame)")
        keep("chat-long-end-held")
    }

    /// A chat that fits on the screen, at its end, while the accessibility audit tries larger
    /// text: its rows swell past the screen for a moment and shrink back (`-rowsSwellEvery`).
    /// The end stays in sight. On CI the audit of the chat found it under the composer (PR #71's
    /// runs): the end was placed again while the rows were tall, and still asserted once they
    /// were short again, a positional move on a chat that fits.
    func testAChatThatFitsStaysAtItsEndWhenItsRowsSwell() {
        launch(["-open", "Camille", "-say", "Up by the north path", "-quietDemo", "YES", "-rowsSwellEvery", "4"])
        XCTAssertTrue(inChat(app).waitForExistence(timeout: 60))
        XCTAssertTrue(encrypted(app).waitForExistence(timeout: 90), "no session with Camille")
        let later = app.buttons[tr("Later")]
        if later.waitForExistence(timeout: 10) { later.tap() }
        let answer = containing("Copy: \"Up by the north path\"")
        wait(answer, 60, "no answer from Camille")
        // Two swells; then at rest (a swell lasts 0.6 s of every 4).
        pause(9)
        let end = app.descendants(matching: .any)["chat.end"]
        let composer = inChat(app)
        for _ in 0..<6 where !(end.exists && end.frame.minY <= composer.frame.minY && answer.isHittable) { pause(0.5) }
        XCTAssertLessThanOrEqual(end.frame.minY, composer.frame.minY, "the end is under the composer: \(end.frame) vs \(composer.frame)")
        XCTAssertTrue(answer.isHittable, "her answer is not on screen")
        keep("chat-fits-end-held")
    }

    /// A long chat at its end: a message just above the screen is reached as VoiceOver or XCTest
    /// reaches it, scrolled into view by accessibility, then long-pressed. The view used to be
    /// held at the end by its scroll position, which undid such a scroll at once (DM-3 on CI:
    /// "Computed hit point {-1, -1} after scrolling to visible", three times).
    func testAMessageAboveIsReachedFromTheEnd() {
        launch(["-open", "Camille", "-fill", "250", "-quietDemo", "YES"])
        XCTAssertTrue(inChat(app).waitForExistence(timeout: 60))
        XCTAssertTrue(encrypted(app).waitForExistence(timeout: 90), "no session with Camille")
        // Ours goes: the view at the end.
        send("At the end now")
        pause(2)
        // A message laid out just above the screen: out of reach until scrolled into view. Read
        // from the frames: isHittable fails the test outright on a row whose frame is not
        // settled yet (a simulator just booted, CI).
        let top = max(app.scrollViews["chat.messages"].frame.minY, app.navigationBars.firstMatch.frame.maxY)
        let above = (200...250).reversed().map { app.staticTexts["Earlier message \($0)"] }
            .first { $0.exists && !$0.frame.isEmpty && $0.frame.maxY <= top }
        guard let target = above else { return XCTFail("no message laid out above the screen") }
        menu(target, tr("Info"))
        wait(app.navigationBars[tr("Info")], 5, "no Info for the message above")
    }

    /// A long chat with a photo, opened again once everything is read, shows its last message.
    func testALongChatOpensAtTheEnd() {
        launch(["-open", "Camille", "-fill", "250"])
        XCTAssertTrue(inChat(app).waitForExistence(timeout: 60))
        XCTAssertTrue(encrypted(app).waitForExistence(timeout: 90), "no session with Camille")
        // Her photo comes about two minutes in.
        let photo = containing("The view from the refuge tonight")
        XCTAssertTrue(photo.waitForExistence(timeout: 240), "no photo")
        // Two minutes of the demo's time at speed 4: everything read, the photo's bubble grown.
        sleep(8)
        app.navigationBars.buttons.element(boundBy: 0).tap()
        let row = app.buttons.containing(NSPredicate(format: "label CONTAINS 'Camille'")).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 10))
        // Nothing unread, or the chat opens on its divider, as it should: her photo itself (and its
        // announcement) can come after its caption, once we are back on the list. Read, then back.
        let unread = tr("%lld unread", 1).replacingOccurrences(of: "1 ", with: "")
        for _ in 0..<3 where row.label.contains(unread) {
            row.tap()
            XCTAssertTrue(inChat(app).waitForExistence(timeout: 10))
            sleep(3)
            app.navigationBars.buttons.element(boundBy: 0).tap()
            XCTAssertTrue(row.waitForExistence(timeout: 10))
        }
        XCTAssertFalse(row.label.contains(unread), "still unread: \(row.label)")
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

    /// B-ios-28: a long chat opens on its unread divider, and her message arriving just after
    /// leaves it there, with the pill. It used to jump up near the top of the loaded page: at
    /// the wall clock's pace her message lands after the opening's last scroll. Then the pill
    /// goes to the end.
    func testALongChatStaysOnItsDividerWhenAMessageArrives() {
        launch(["-open", "Camille", "-fill", "5000", "-quietDemo", "YES"], speed: 1)
        XCTAssertTrue(inChat(app).waitForExistence(timeout: 60))
        XCTAssertTrue(encrypted(app).waitForExistence(timeout: 90), "no session with Camille")
        let divider = app.staticTexts.matching(NSPredicate(format: "label ENDSWITH %@", tr("%lld unread messages", 5).replacingOccurrences(of: "5 ", with: ""))).firstMatch
        XCTAssertTrue(divider.waitForExistence(timeout: 30), "no unread divider")
        let pill = app.buttons.matching(NSPredicate(format: "label CONTAINS %@", tr("%lld new messages", 1).replacingOccurrences(of: "1 ", with: ""))).firstMatch
        XCTAssertTrue(pill.waitForExistence(timeout: 60), "no pill for her message")
        pause(4)
        let shown = app.staticTexts.matching(NSPredicate(format: "label BEGINSWITH 'Earlier message '")).allElementsBoundByIndex
            .first { $0.isHittable }?.label ?? "-"
        XCTAssertTrue(divider.isHittable, "her message moved the view off the divider: now on “\(shown)”")
        keep("chat-long-divider-held")
        pill.tap()
        pause(2)
        let end = app.descendants(matching: .any)["chat.end"]
        XCTAssertTrue(end.exists && end.frame.minY <= composer.frame.minY, "the pill did not reach the end")
    }

    /// B-ios-28: in a long chat, a message arriving moves the view only when the end is on
    /// screen. Read above, on the earlier page, her answer comes: the page's first message stays
    /// at the top and the pill counts it; the pill then goes to the end, her answer on it.
    func testALongChatHoldsItsPlaceWhenAMessageArrives() {
        // Her answer 20 s of the demo's time after our text reaches her: once we read above.
        launch(["-open", "Camille", "-fill", "5000", "-quietDemo", "YES", "-answerAfter", "20"])
        XCTAssertTrue(inChat(app).waitForExistence(timeout: 60))
        XCTAssertTrue(encrypted(app).waitForExistence(timeout: 90), "no session with Camille")
        let list = app.scrollViews["chat.messages"]
        let pill = app.buttons.matching(NSPredicate(format: "label CONTAINS %@", tr("%lld new messages", 1).replacingOccurrences(of: "1 ", with: ""))).firstMatch
        let end = app.descendants(matching: .any)["chat.end"]
        send("And the path down?")
        // Up to the earlier page (flings on the messages from their upper part: a swipe on the
        // keyboard, up after a text, types words).
        let earlier = button(tr("Show earlier messages"))
        for _ in 0..<30 where !(earlier.exists && earlier.isHittable) {
            list.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.1))
                .press(forDuration: 0.01, thenDragTo: list.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.9)),
                       withVelocity: .fast, thenHoldForDuration: 0)
        }
        let top = app.staticTexts["Earlier message 4901"]
        earlier.tap()
        wait(top, 5, "the earlier page did not load")
        XCTAssertTrue(top.isHittable, "the earlier page moved the view")
        // Her answer: the view stays, the pill counts it.
        wait(pill, 60, "no pill for her answer")
        pause(2)
        XCTAssertTrue(top.isHittable, "her answer moved the view off the earlier page")
        keep("chat-long-held")
        // The pill: the end, her answer on it.
        pill.tap()
        let answer = containing("Copy: \"And the path down?\"")
        for _ in 0..<10 where !(end.exists && end.frame.minY <= composer.frame.minY && answer.exists && answer.isHittable) { pause(0.5) }
        XCTAssertTrue(end.exists && end.frame.minY <= composer.frame.minY, "the pill did not reach the end")
        XCTAssertTrue(answer.isHittable, "her answer is not on screen")
    }


    /// A long chat (a lazy list since M5) keeps its places: it opens on the unread divider, the
    /// page before keeps the message that was at the top, and the pill of a new message goes
    /// down to it.
    func testALongChatKeepsItsPlaces() {
        launch(["-open", "Camille", "-fill", "5000"])
        XCTAssertTrue(inChat(app).waitForExistence(timeout: 60))
        XCTAssertTrue(encrypted(app).waitForExistence(timeout: 90), "no session with Camille")
        let divider = app.staticTexts.matching(NSPredicate(format: "label ENDSWITH %@", tr("%lld unread messages", 5).replacingOccurrences(of: "5 ", with: ""))).firstMatch
        XCTAssertTrue(divider.waitForExistence(timeout: 10), "no unread divider")
        XCTAssertTrue(divider.isHittable, "the divider is off screen")
        // Up to the first message of the page, then the page before it.
        let list = app.scrollViews.firstMatch
        let earlier = button(tr("Show earlier messages"))
        for _ in 0..<20 where !(earlier.exists && earlier.isHittable) { list.swipeDown(velocity: .fast) }
        XCTAssertTrue(earlier.isHittable, "no earlier page")
        let top = app.staticTexts["Earlier message 4901"]
        XCTAssertTrue(top.exists)
        earlier.tap()
        sleep(2)
        XCTAssertTrue(top.isHittable, "the earlier page moved the view")
        XCTAssertTrue(app.staticTexts["Earlier message 4900"].exists, "the page before is not there")
        // Camille's first message comes while we read above: the pill, then the end.
        let pill = app.buttons.matching(NSPredicate(format: "label CONTAINS %@", tr("%lld new messages", 1).replacingOccurrences(of: "1 ", with: ""))).firstMatch
        XCTAssertTrue(pill.waitForExistence(timeout: 180), "no pill")
        pill.tap()
        sleep(2)
        let end = app.descendants(matching: .any)["chat.end"]
        XCTAssertTrue(end.exists && end.frame.minY <= inChat(app).frame.minY, "not at the end")
    }
}

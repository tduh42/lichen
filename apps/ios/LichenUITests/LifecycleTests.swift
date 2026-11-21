import XCTest

/// Lifecycle (docs/testing.md §10): notifications, mute, archive, pin, deletion, background.
/// LC-6 is ContactsTests' list check.
final class LifecycleTests: LichenUITest {
    private var springboard: XCUIApplication { XCUIApplication(bundleIdentifier: "com.apple.springboard") }

    /// A banner whose text holds `text`.
    private func banner(_ text: String) -> XCUIElement {
        springboard.descendants(matching: .any).matching(NSPredicate(format: "label CONTAINS %@", text)).firstMatch
    }

    /// The chat on screen is `name`'s (its bar's title).
    private func inChat(_ name: String, file: StaticString = #filePath, line: UInt = #line) {
        let title = app.navigationBars.firstMatch.descendants(matching: .any)
            .matching(NSPredicate(format: "label BEGINSWITH %@", name)).firstMatch
        wait(title, 10, "\(name)'s chat did not open", file: file, line: line)
    }

    /// A row's menu (a long press, once the list stays still: a row that moves under the press
    /// opens another's menu), then `action`.
    private func rowAction(_ name: String, _ action: String, file: StaticString = #filePath, line: UInt = #line) {
        let r = wait(row(name), 10, "no row for \(name)", file: file, line: line)
        var at = r.frame
        for _ in 0..<10 {
            pause(0.7)
            if r.frame == at { break }
            at = r.frame
        }
        r.press(forDuration: 1.5)
        wait(app.buttons[action], 5, "no \(action)", file: file, line: line).tap()
    }

    /// Waits for a notification about `chat`, taps it: its conversation opens.
    private func openFromNotification(_ chat: String, _ timeout: TimeInterval, file: StaticString = #filePath, line: UInt = #line) {
        let b = banner(chat)
        XCTAssertTrue(b.waitForExistence(timeout: timeout), "no notification for \(chat)", file: file, line: line)
        b.tap()
        inChat(chat, file: file, line: line)
        keep("lc-notification-" + chat.lowercased().replacingOccurrences(of: " ", with: "-"))
    }

    /// LC-1: a notification opens its conversation, for each kind (one launch each: at speed 4
    /// the demo's messages come close together). A lichen chat's is ChatNavigationTests'.
    func testLC1_MeshCoreNotification() {
        launch(banners: true)
        openFromNotification("Marc", 60)
    }

    /// LC-1: a channel's, Lucie's post on Public.
    func testLC1_ChannelNotification() {
        launch(banners: true)
        openFromNotification("Public", 90)
    }

    /// LC-1: a room server's, a post on the Refuge board (logged in).
    func testLC1_RoomNotification() {
        launch(["-roomLogin", "hello"], banners: true)
        openFromNotification("Refuge board", 120)
    }

    /// LC-1: a group's, Camille's answer to our post in Rando Vercors, Messages on screen.
    func testLC1_GroupNotification() {
        launch(["-open", "Rando Vercors"], banners: true)
        wait(composer, 90, "the group did not open")
        send("Who is at the refuge?")
        back()
        openFromNotification("Rando Vercors", 120)
    }

    /// LC-2: pinned, Marc's chat goes on top; archived, Public leaves Messages and comes back
    /// with Lucie's next post; muted, Camille's answer gives no notification.
    func testLC2_PinArchiveMute() {
        // At the wall clock's pace: Lucie's next post comes a minute after the first.
        launch(speed: 1, banners: true)
        wait(row("Marc"), 90, "no message from Marc")
        wait(row("Camille"), 5)
        rowAction("Marc", tr("Pin"))
        let rows = app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Marc,' OR label BEGINSWITH 'Camille,'"))
        XCTAssertTrue(rows.element(boundBy: 0).label.hasPrefix("Marc,"), "Marc is not on top")
        // Archived: gone, then back with Lucie's next post (110 s into the demo).
        wait(row("Public"), 30, "no Public")
        rowAction("Public", tr("Archive"))
        waitGone(row("Public"), 5, "Public stayed in Messages")
        keep("lc-archived")
        wait(row("Public"), 120, "Public did not come back with a new post")
        // Muted: her answer comes without a notification.
        rowAction("Camille", tr("Mute"))
        wait(app.buttons[tr("Until I turn it back on")], 5, "no mute durations").tap()
        row("Camille").tap()
        send("Quiet for a while")
        back()
        let answer = banner("Copy: \"Quiet for a while")
        let deadline = Date().addingTimeInterval(120)
        while !row("Camille").label.contains("Copy: \"Quiet") && Date() < deadline {
            XCTAssertFalse(answer.exists, "a notification for a muted chat")
            pause(1)
        }
        XCTAssertTrue(row("Camille").label.contains("Copy: \"Quiet"), "her answer did not come")
    }

    /// LC-3: Camille's conversation deleted: gone from Messages; a new message starts it again,
    /// without the old ones.
    func testLC3_DeleteAConversation() {
        launch(["-quietDemo", "YES"])
        let first = "Hi! I'm at the refuge"
        wait(row("Camille"), Self.camilleTimeout, "no message from Camille")
        // Marc's message too, which moves the rows.
        wait(row("Marc"), 30, "no message from Marc")
        rowAction("Camille", tr("Delete conversation"))
        let confirm = app.buttons.matching(NSPredicate(format: "label == %@", tr("Delete"))).firstMatch
        if confirm.waitForExistence(timeout: 3) { confirm.tap() }
        waitGone(row("Camille"), 5, "her conversation stayed")
        // From New message: her chat, empty, then a text and her answer.
        button(tr("New message")).tap()
        wait(app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Camille'")).firstMatch, 10).tap()
        wait(composer, 10)
        XCTAssertFalse(containing(first).exists, "the deleted messages came back")
        send("Starting again")
        waitText("Copy: \"Starting again\"", 60)
        back()
        wait(row("Camille"), 5, "no row for the new conversation")
    }

    /// LC-4: a restart keeps messages, contacts, groups, preferences and a waiting request. On
    /// a radio over TCP (FakeTcpRadio, the app's real link and store): the demo starts afresh at
    /// each launch, by design.
    func testLC4_RestartKeepsEverything() throws {
        let radio = try FakeTcpRadio()
        defer { radio.stop() }
        launch(["-tcp", radio.address, "-freshInstall", "YES"], demo: false)
        // Ours: an answer to Marc, his chat pinned, a group.
        wait(row("Marc"), 90, "no message from Marc")
        row("Marc").tap()
        send("Two of us tonight")
        back()
        rowAction("Marc", tr("Pin"))
        button(tr("New message")).tap()
        let name = app.textFields[tr("Group name")]
        for _ in 0..<8 where !(name.exists && name.isHittable) { app.collectionViews.firstMatch.swipeUp() }
        name.tap()
        name.typeText("Ridge walk")
        let create = button(tr("Create group"))
        if !create.isHittable { reveal(create); pause(1) }
        create.tap()
        wait(composer, 10, "the group did not open")
        back()
        // Camille's first message: a request (requests are on outside the demo).
        let request = containing(tr("Someone using %@'s radio wants to write to you", "Camille"))
        wait(request, 90, "no request from Camille")
        keep("lc-before-restart")
        relaunch(["-tcp", radio.address], onboarded: true)
        // At once, before the radio answers: the conversations, the pin, the group, the request.
        let marc = wait(row("Marc"), 10, "Marc's chat is gone")
        XCTAssertTrue(marc.label.contains("Two of us tonight"), "our answer is gone: \(marc.label)")
        let group = wait(row("Ridge walk"), 5, "the group is gone")
        XCTAssertLessThan(marc.frame.minY, group.frame.minY, "Marc's chat is no longer pinned on top")
        XCTAssertTrue(request.exists, "Camille's request is gone")
        keep("lc-after-restart")
        // The radio's contacts too, once it answers.
        button(tr("New message")).tap()
        wait(app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Marc'")).firstMatch, 30, "Marc is not a contact")
    }

    /// LC-5: the app sent to the background right after a text, then back: her answer is there,
    /// once, and ours once.
    func testLC5_BackgroundAndBack() {
        launch(["-open", "Camille", "-quietDemo", "YES"])
        openedCamille()
        send("Still there?")
        XCUIDevice.shared.press(.home)
        pause(15)
        app.activate()
        waitText("Copy: \"Still there?\"", 60)
        pause(5)
        XCTAssertEqual(bubbles("Still there?"), 1, "our text twice")
        // Her bubble (its label ends with its time; its text, exposed alone, does not).
        XCTAssertEqual(app.scrollViews.descendants(matching: .any)
            .matching(NSPredicate(format: "label BEGINSWITH 'Copy: \"Still there?' AND label MATCHES '.*, [0-9]{2}:[0-9]{2}.*'")).count,
                       1, "her answer twice")
    }

    /// LC-6: every conversation with a message has its row, whatever the contact list says:
    /// a lichen chat (Camille), a MeshCore chat (Marc, filed by the radio as a repeater, so not
    /// one of the people), a channel (Public), a group (Rando Vercors) and a room server (the
    /// Refuge board, logged in).
    func testLC6_EveryConversationListed() {
        launch(["-roomLogin", "hello", "-marcRepeater", "YES"])
        for (chat, timeout) in [("Camille", 60.0), ("Marc", 60.0), ("Public", 60.0), ("Refuge board", 60.0), ("Rando Vercors", 120.0)] {
            wait(row(chat), timeout, "no row for \(chat)")
        }
        keep("lc-every-row")
        button(tr("New message")).tap()
        wait(app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Camille'")).firstMatch, 10)
        let marcAsPerson = app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Marc' AND (label CONTAINS %@ OR label CONTAINS %@)",
            ", " + tr("lichen") + ",", tr("MeshCore chat · not lichen-encrypted"))).firstMatch
        XCTAssertFalse(marcAsPerson.exists, "Marc listed with the people")
    }
}

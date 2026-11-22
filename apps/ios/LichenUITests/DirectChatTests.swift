import XCTest

/// Direct lichen chats (docs/testing.md §4), with Camille on the demo network. She answers each
/// text with "Copy: "<text>". I'm at the refuge, one hop away." about 4 s after it reaches her,
/// and gives it a 👍 4 s later: her side of the conversation, seen from ours.
final class DirectChatTests: LichenUITest {
    private let first = "Hi! I'm at the refuge"
    /// Her first message whole (it holds a comma: `bubbles` needs the whole text).
    private let firstWhole = "Hi! I'm at the refuge, the mobile network doesn't reach up here."

    /// Her chat open, her session up; the demo quiet (no group, photo or channel posts on the
    /// air: her answers come in seconds) unless `quiet` is false.
    private func openCamille(_ arguments: [String] = [], quiet: Bool = true, file: StaticString = #filePath, line: UInt = #line) {
        launch(["-open", "Camille"] + (quiet ? ["-quietDemo", "YES"] : []) + arguments)
        openedCamille(file: file, line: line)
    }

    /// Her answer to `text`.
    private func copy(_ text: String) -> String { "Copy: \"\(text)\"" }

    /// DM-1: a text goes, is delivered, shows once; she got it whole.
    func testDM1_SendAText() {
        openCamille()
        let text = "On my way up, see you tonight"
        send(text)
        let sent = wait(message(text), 5, "not shown")
        XCTAssertEqual(composer.value as? String ?? "", tr("Message"), "the composer kept the text")
        waitLabel(sent, contains: tr("Delivered"), 60)
        waitText(copy(text), 45)
        XCTAssertEqual(bubbles(text), 1, "shown more than once")
        keep("dm-sent")
    }

    /// DM-2 and DM-12: her first message makes her row, with an unread badge, the tab's badge
    /// and a banner; opening her chat reads it.
    func testDM2_ReceiveAText() {
        launch(banners: true)
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        let banner = springboard.descendants(matching: .any)
            .matching(NSPredicate(format: "label CONTAINS 'Camille' AND label CONTAINS 'refuge'")).firstMatch
        XCTAssertTrue(banner.waitForExistence(timeout: Self.camilleTimeout), "no notification")
        let camille = wait(row("Camille"), 5)
        XCTAssertTrue(camille.label.contains(first), "no preview: \(camille.label)")
        XCTAssertTrue(camille.label.contains(tr("%lld unread", 1)), "no unread badge: \(camille.label)")
        let messagesTab = app.tabBars.buttons.element(boundBy: 0)
        XCTAssertFalse((messagesTab.value as? String ?? "").isEmpty, "no badge on the tab")
        keep("dm-received")
        camille.tap()
        wait(composer, 5)
        waitText(first, 5)
        back()
        let read = wait(row("Camille"), 5)
        XCTAssertFalse(read.label.contains(tr("%lld unread", 1)), "still unread: \(read.label)")
    }

    /// DM-2, in an open chat: her first message, arriving while her conversation is on
    /// screen (opened empty), shows there. It used not to: only its day separator was laid out.
    func testDM2_AFirstMessageShowsInAnOpenChat() {
        openCamille()
        wait(message(first), 20, "her first message did not show in her open chat")
        XCTAssertTrue(message(first).isHittable, "her first message is off screen")
        XCTAssertTrue(app.descendants(matching: .any)["chat.end"].exists, "the end of the conversation is not laid out")
        keep("dm-first-in-open-chat")
    }

    /// DM-12: a badge per row and on the tab, and opening a chat clears its own.
    func testDM12_UnreadCountsAndMarkRead() {
        launch()
        // Camille, Lucie on Public, then Marc: one unread each.
        let marc = wait(row("Marc"), Self.camilleTimeout + 20, "no message from Marc")
        let camille = wait(row("Camille"), 5)
        let pub = wait(row("Public"), 5)
        for r in [camille, pub, marc] { waitLabel(r, contains: tr("%lld unread", 1), 10) }
        let tabBadge = { self.app.tabBars.buttons.element(boundBy: 0).value as? String ?? "" }
        XCTAssertTrue(tabBadge().contains("3"), "tab badge: \(tabBadge())")
        camille.tap()
        wait(composer, 5)
        back()
        XCTAssertFalse(row("Camille").label.contains(tr("%lld unread", 1)), "Camille still unread")
        XCTAssertTrue(row("Public").label.contains(tr("%lld unread", 1)), "Public was read too")
        XCTAssertTrue(tabBadge().contains("2"), "tab badge after reading Camille: \(tabBadge())")
    }

    /// DM-3: a long text (several fragments) arrives whole: her answer quotes all of it.
    func testDM3_LongText() {
        openCamille()
        let words = (1...120).map { "w\($0)" }.joined(separator: " ")
        let text = "Long one: \(words) END"
        XCTAssertGreaterThan(text.utf8.count, 400)
        UIPasteboard.general.string = text
        composer.tap()
        composer.press(forDuration: 1.2)
        let paste = app.menuItems.matching(NSPredicate(format: "label IN %@", ["Paste", "Coller"])).firstMatch
        wait(paste, 5, "no Paste").tap()
        button(tr("Send")).tap()
        waitLabel(message("Long one: w1 w2"), contains: tr("Delivered"), 120)
        let answer = waitText(copy(text), 120)
        XCTAssertTrue(answer.label.contains("w119 w120 END"), "her answer is cut: \(answer.label.suffix(80))")
        // Several fragments, said in Info.
        // The bubble (its text alone is taller than the screen's free part, keyboard up).
        menu(message("Long one: w1 w2"), tr("Info"))
        // "Fragments, 5" (a labelled value reads as one).
        let fragments = wait(containing(tr("Fragments")), 5, "no fragments in Info")
        let count = Int(fragments.label.components(separatedBy: CharacterSet.decimalDigits.inverted).joined()) ?? 0
        XCTAssertGreaterThan(count, 1, "a long text in one fragment: \(fragments.label)")
        keep("dm-long-info")
    }

    /// DM-4: a reply shows what it answers; tapping the quote goes to it.
    func testDM4_Reply() {
        openCamille(["-reply", "See you there"])
        let reply = waitText("See you there", Self.camilleTimeout)
        let quote = wait(containing(tr("In reply to %@: %@", "Camille", "")).exists
                         ? containing(tr("In reply to %@: %@", "Camille", "")) : containing(first), 5)
        XCTAssertTrue(quote.label.contains(first), "the quote: \(quote.label)")
        waitLabel(reply, contains: tr("Delivered"), 60)
        keep("dm-reply")
        // One of ours, from the menu: the bar above the composer, then the quote.
        menu(textOf(firstWhole), tr("Reply"))
        wait(button(tr("Cancel the reply")), 5, "no reply bar")
        composer.tap()
        composer.typeText("Two of us")
        button(tr("Send")).tap()
        waitGone(button(tr("Cancel the reply")), 5)
        let quotes = app.descendants(matching: .any).matching(NSPredicate(format: "label BEGINSWITH %@", tr("In reply to %@: %@", "Camille", "").trimmingCharacters(in: .whitespaces)))
        XCTAssertGreaterThanOrEqual(quotes.count, 2, "the second reply has no quote")
        // Camille answers texts, not replies: delivered is her side.
        wait(bubble("Two of us", with: tr("Delivered")), 60, "the reply was not delivered")
        // The quote, tapped, goes to her message.
        quotes.firstMatch.tap()
        pause(1)
        XCTAssertTrue(message(first).isHittable, "the quoted message is not on screen")
    }

    /// DM-5: a reaction of ours added, changed, removed; hers on our text.
    func testDM5_Reactions() {
        openCamille()
        wait(textOf(firstWhole), 30, "no message from Camille")
        menu(textOf(firstWhole), tr("React"))
        wait(button("👍"), 5).tap()
        wait(element(tr("%@, %u, including yours", "👍", UInt32(1))), 10, "no 👍 of ours")
        menu(textOf(firstWhole), tr("React"))
        wait(button("❤️"), 5).tap()
        wait(element(tr("%@, %u, including yours", "❤️", UInt32(1))), 10, "no ❤️ of ours")
        XCTAssertFalse(element(tr("%@, %u, including yours", "👍", UInt32(1))).exists, "the 👍 stayed")
        menu(textOf(firstWhole), tr("React"))
        wait(button(tr("Remove my reaction")), 5).tap()
        waitGone(element(tr("%@, %u, including yours", "❤️", UInt32(1))), 10, "the reaction stayed")
        // Hers, on ours.
        send("Soup tonight?")
        wait(element(tr("%@, %u", "👍", UInt32(1))), 60, "no 👍 from Camille")
        keep("dm-reactions")
    }

    /// DM-6: an edited text shows its new words and the mark.
    func testDM6_EditASentMessage() {
        openCamille()
        send("Dinner at 7")
        waitLabel(message("Dinner at 7"), contains: tr("Delivered"), 60)
        menu(textOf("Dinner at 7"), tr("Edit"))
        wait(element(tr("Editing your message")), 5, "not editing")
        XCTAssertEqual(composer.value as? String, "Dinner at 7", "the draft is not the message")
        composer.typeText(XCUIKeyboardKey.delete.rawValue + "8")
        button(tr("Send")).tap()
        let edited = wait(message("Dinner at 8"), 10, "the edit is not shown")
        waitLabel(edited, contains: tr("edited"), 10)
        XCTAssertFalse(message("Dinner at 7").exists, "the old text stayed")
        keep("dm-edited")
    }

    /// DM-7: deleted for me, gone; deleted for everyone, its place says so.
    func testDM7_DeleteForMeAndForEveryone() {
        openCamille()
        send("Only for me")
        waitLabel(message("Only for me"), contains: tr("Delivered"), 60)
        menu(textOf("Only for me"), tr("Delete for me"))
        wait(app.buttons.matching(NSPredicate(format: "label == %@", tr("Delete for me"))).element(boundBy: 0), 5).tap()
        waitGone(message("Only for me"), 5, "deleted for me, still there")
        send("For everyone")
        waitLabel(message("For everyone"), contains: tr("Delivered"), 60)
        menu(textOf("For everyone"), tr("Delete for everyone"))
        wait(app.buttons.matching(NSPredicate(format: "label == %@", tr("Delete for everyone"))).element(boundBy: 0), 5).tap()
        waitGone(message("For everyone"), 10, "deleted for everyone, still there")
        keep("dm-deleted")
    }

    /// DM-8: with a timer, our text and her answer disappear once it runs out.
    func testDM8_DisappearingTimer() {
        openCamille(["-timer", "50"])
        let timer = app.navigationBars.buttons.matching(NSPredicate(format: "label BEGINSWITH %@",
            tr("Disappearing messages: %@", "#").components(separatedBy: "#")[0])).firstMatch
        wait(timer, 5)
        XCTAssertNotEqual(timer.label, tr("Disappearing messages: off"))
        send("Gone soon")
        waitLabel(message("Gone soon"), contains: tr("Delivered"), 40)
        let answer = waitText(copy("Gone soon"), 40)
        keep("dm-timer")
        waitGone(message("Gone soon"), 60, "our text stayed past its timer")
        waitGone(answer, 60, "her answer stayed past the timer")
        // What is left still shows: the session line at least.
        pause(2)
        XCTAssertTrue(containing(tr("Encrypted session · key to verify")).isHittable, "the chat went blank")
        keep("dm-timer-after")
    }

    /// DM-9: older pages load above, in order, each message once.
    func testDM9_PagingOlderMessages() {
        openCamille(["-fill", "250"])
        let list = app.scrollViews.firstMatch
        let earlier = button(tr("Show earlier messages"))
        for _ in 0..<25 where !(earlier.exists && earlier.isHittable) { list.swipeDown(velocity: .fast) }
        XCTAssertTrue(earlier.isHittable, "no earlier page")
        let top = message("Earlier message 151")
        XCTAssertTrue(top.exists, "the last page does not start at 151")
        XCTAssertFalse(message("Earlier message 150").exists, "150 already there")
        earlier.tap()
        wait(message("Earlier message 150"), 5, "the earlier page did not load")
        XCTAssertTrue(top.isHittable, "the earlier page moved the view")
        // In order: 150 above 151, 149 above 150.
        XCTAssertLessThan(message("Earlier message 150").frame.minY, top.frame.minY)
        XCTAssertEqual(bubbles("Earlier message 150"), 1, "150 not once")
        XCTAssertEqual(bubbles("Earlier message 151"), 1, "151 not once")
        keep("dm-paging")
    }

    /// DM-10: a search finds messages in every chat; a result opens its chat on it.
    func testDM10_SearchMessages() {
        launch()
        wait(row("Marc"), Self.camilleTimeout + 30, "no message from Marc")
        wait(row("Camille"), 5)
        button(tr("Search messages")).tap()
        let field = wait(app.textFields[tr("Search messages")], 5, "no search field")
        field.typeText("refuge")
        let fromCamille = wait(app.buttons.containing(NSPredicate(format: "label CONTAINS %@", first)).firstMatch, 5, "no result from Camille")
        let fromMarc = wait(app.buttons.containing(NSPredicate(format: "label CONTAINS 'Marc from the refuge'")).firstMatch, 5,
                            "no result from Marc")
        XCTAssertTrue(fromCamille.exists && fromMarc.exists)
        keep("dm-search")
        fromMarc.tap()
        wait(composer, 10, "the result did not open its chat")
        wait(app.navigationBars.firstMatch.descendants(matching: .any).matching(NSPredicate(format: "label BEGINSWITH 'Marc'")).firstMatch, 5)
        // Still there once its highlight has gone, 2.6 s after it opened (B-ios-27: this chat
        // of one message went blank then).
        pause(4)
        XCTAssertTrue(message("Hi! Marc from the refuge").isHittable, "not on the result")
    }

    /// DM-11: long press › Info on a delivered text: when, status, fragments, airtime.
    func testDM11_MessageInfo() {
        launch(["-open", "Camille", "-say", "Info please", "-screen", "info", "-quietDemo", "YES"])
        wait(app.navigationBars[tr("Info")], Self.camilleTimeout + 60, "no Info sheet")
        for label in ["Sent", "Status", "Fragments", "Airtime"] {
            XCTAssertTrue(element(tr(label)).exists, "no \(label)")
        }
        XCTAssertTrue(containing(tr("Delivered")).exists, "not delivered")
        keep("dm-info")
        button(tr("Done")).tap()
        wait(composer, 5)
    }

    /// DM-13: a text forwarded to Marc (MeshCore) arrives there; he answers.
    func testDM13_ForwardAMessage() {
        openCamille()
        wait(textOf(firstWhole), 30, "no message from Camille")
        menu(textOf(firstWhole), tr("Forward"))
        // Marc, under MeshCore, once his radio is known (the sheet lists him by name).
        let marc = wait(app.buttons.matching(NSPredicate(format: "label == 'Marc' OR label ENDSWITH ', Marc'")).firstMatch, 20,
                        "Marc is not offered: \(app.buttons.allElementsBoundByIndex.map(\.label))")
        marc.tap()
        back()
        openChat("Marc", 10)
        let forwarded = wait(message(first), 10, "not in Marc's chat")
        waitLabel(forwarded, contains: tr("Delivered"), 60)
        waitText("It works! See you tonight", 60)
        XCTAssertEqual(bubbles(firstWhole), 1, "forwarded more than once")
        keep("dm-forwarded")
    }

    /// DM-13, D-2: a blocked contact is not offered to forward to, as when sharing.
    func testDM13_ForwardLeavesBlockedContactsOut() {
        openCamille()
        // Camille blocked, then Marc's message to forward.
        button(tr("More")).tap()
        button(tr("Block")).tap()
        wait(app.buttons.matching(NSPredicate(format: "label == %@", tr("Block"))).element(boundBy: 0), 5).tap()
        wait(containing(tr("You blocked %@. Nothing they send reaches this phone.", "Camille")), 5, "no blocked bar")
        back()
        openChat("Marc", 30)
        let marc = "Hi! Marc from the refuge. Are you on lichen? I only have the MeshCore app."
        wait(textOf(marc), 10, "no message from Marc")
        menu(textOf(marc), tr("Forward"))
        wait(app.navigationBars[tr("Forward")], 5, "no Forward sheet")
        XCTAssertTrue(app.buttons.matching(NSPredicate(format: "label == 'Public' OR label ENDSWITH ', Public'")).firstMatch.exists,
                      "the sheet lists nothing")
        XCTAssertFalse(app.buttons.matching(NSPredicate(format: "label == 'Camille' OR label ENDSWITH ', Camille'")).firstMatch.exists,
                       "a blocked contact is offered")
        // Nobody else with lichen: no Contacts section, rather than its title alone.
        XCTAssertFalse(app.staticTexts[tr("Contacts")].exists, "an empty Contacts section")
        keep("dm-forward-blocked")
    }

    /// DM-14: a link and a place in a text: copied whole; the place opens the map.
    func testDM14_CopyLinksAndPlaces() {
        openCamille()
        let text = "Meet at 44.8960, 5.4880 see https://example.org/refuge"
        send(text)
        let sent = wait(message("Meet at"), 5)
        waitLabel(sent, contains: tr("Delivered"), 60)
        menu(textOf(text), tr("Copy"))
        composer.tap()
        composer.press(forDuration: 1.2)
        wait(app.menuItems.matching(NSPredicate(format: "label IN %@", ["Paste", "Coller"])).firstMatch, 5).tap()
        XCTAssertEqual(composer.value as? String, text, "copied text")
        // The place: Show on the map.
        menu(textOf(text), tr("Show on the map"))
        wait(app.tabBars.buttons.element(boundBy: 1), 5)
        XCTAssertTrue(app.tabBars.buttons.element(boundBy: 1).isSelected, "the map did not open")
        keep("dm-place-map")
    }
}

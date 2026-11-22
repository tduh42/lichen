import SQLite3
import XCTest

/// MeshCore chats and channels (docs/testing.md §5), with the demo's stock-app users: Marc, who
/// writes in plain and answers once, and Lucie, who posts on Public.
final class MeshCoreTests: LichenUITest {
    private let marc = "Hi! Marc from the refuge. Are you on lichen? I only have the MeshCore app."

    /// MC-1: Marc's plain message arrives in his MeshCore chat, marked as such; our answer
    /// reaches his radio, and he answers.
    func testMC1_PlainChatWithAStockUser() {
        // The quiet demo: Marc answers once, and a busy air (the group, the photo, the
        // channel's posts) can lose his answer even after the two more tries his app gives it.
        launch(["-open", "Marc", "-sayPlain", "Yes, both: lichen and MeshCore", "-quietDemo", "YES"])
        wait(composer, 30, "his chat did not open")
        wait(textOf(marc), 30, "no message from Marc")
        XCTAssertTrue(containing(tr("MeshCore · not lichen-encrypted")).exists, "not said to be a MeshCore chat")
        let answer = wait(message("Yes, both: lichen and MeshCore"), 10, "our answer is not shown")
        waitLabel(answer, contains: tr("Delivered"), 60)
        waitText("It works! See you tonight", 60)
        keep("mc-plain")
        back()
        let row = wait(row("Marc"), 5)
        XCTAssertTrue(row.label.contains(tr("MeshCore")), row.label)
    }

    /// MC-2: Public: Lucie's post with her name; a post of ours goes out to the channel.
    func testMC2_PublicChannelReadAndPost() {
        launch(["-open", "Public"])
        wait(composer, 30, "Public did not open")
        wait(containing("Fog on the Pas de la Ville"), 40, "no post from Lucie")
        XCTAssertTrue(element("Lucie").exists || containing("Lucie").exists, "the post's author is not named")
        XCTAssertTrue(containing(tr("MeshCore channel · not lichen-encrypted")).exists)
        send("Anyone on the ridge tonight?")
        waitLabel(message("Anyone on the ridge tonight?"), contains: tr("Sent"), 30)
        keep("mc-public")
    }

    /// MC-5: mentions and replies (§15.9). Public muted: Lucie's post that mentions our radio is
    /// notified all the same, "Lucie mentioned you in Public"; in the channel it shows
    /// "@ThinkNode M5", no brackets, marked as mentioning us. Marc's reply to her quotes her post,
    /// and the quote leads to it. Ours to her post says whom it answers, and is quoted.
    func testMC5_MentionsAndReplies() {
        let sun = "Sun on the plateau, the path to the refuge is dry."
        // Half speed: Public is muted before Lucie's mention (110 s into the demo).
        launch(speed: 2, banners: true)
        // Public muted, from its row's menu (once the list stays still).
        let publicRow = wait(row("Public"), 60, "no Public")
        var at = publicRow.frame
        for _ in 0..<10 {
            pause(0.7)
            if publicRow.frame == at { break }
            at = publicRow.frame
        }
        publicRow.press(forDuration: 1.5)
        wait(app.buttons[tr("Mute")], 5, "no Mute").tap()
        wait(app.buttons[tr("Until I turn it back on")], 5, "no mute durations").tap()
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        let mentioned = springboard.descendants(matching: .any)
            .matching(NSPredicate(format: "label CONTAINS %@", tr("%@ mentioned you in %@", "Lucie", "Public"))).firstMatch
        XCTAssertTrue(mentioned.waitForExistence(timeout: 120), "no notification for the mention")
        keep("mc-mention-notification")
        // In the notification and in Messages: "@ThinkNode M5", no brackets.
        XCTAssertFalse(mentioned.label.contains("@[ThinkNode M5]"), mentioned.label)
        let preview = wait(row("Public"), 5, "no Public")
        XCTAssertTrue(preview.label.contains("Coming up, @ThinkNode M5?"), preview.label)
        XCTAssertFalse(preview.label.contains("@[ThinkNode M5]"), preview.label)
        row("Public").tap()
        wait(composer, 30, "Public did not open")
        wait(containing("Coming up, @ThinkNode M5?"), 60, "Lucie's mention is not shown")
        XCTAssertFalse(containing("@[ThinkNode M5]").exists, "the mention shows its brackets")
        let marked = app.descendants(matching: .any).matching(NSPredicate(format: "value == %@", tr("Mentions you"))).firstMatch
        XCTAssertTrue(marked.exists, "the mention of us is not marked")
        // Marc's reply, 320 s into the demo.
        wait(containing("@Lucie Thanks!"), 200, "no reply from Marc")
        let quote = wait(element(tr("In reply to %@: %@", "Lucie", sun)), 10, "Marc's reply has no quote")
        keep("mc-mention-quote")
        quote.tap()
        XCTAssertTrue(wait(textOf(sun), 10, "the quote does not lead to her post").isHittable)
        // Ours, to her post.
        menu(textOf(sun), tr("Reply"))
        wait(containing(tr("Replying to %@", "Lucie")), 5, "not said whom it answers")
        send("On my way")
        let ours = wait(bubble("@Lucie On my way", with: tr("Sent")), 60, "our reply did not go")
        XCTAssertTrue(ours.label.contains("Lucie"), ours.label)
        XCTAssertTrue(app.descendants(matching: .any).matching(NSPredicate(format: "label == %@", tr("In reply to %@: %@", "Lucie", sun))).count >= 2,
                      "our reply has no quote")
        keep("mc-mention-reply")
    }

    /// MC-3: a channel's code shows after its warning and holds the channel; a code scanned
    /// (`-scanChannel`, the camera's stand-in) joins its channel.
    func testMC3_ChannelCodeShowAndJoin() throws {
        let code = "meshcore://channel/add?name=Ridge&secret=0123456789abcdef0123456789abcdef"
        launch(["-open", "Public", "-scanChannel", code])
        wait(composer, 30, "Public did not open")
        button(tr("Channel details")).tap()
        cardButton(tr("Share by QR code")).tap()
        wait(containing(tr("Whoever scans it joins")), 5, "no warning before the code")
        XCTAssertFalse(element(tr("QR code of %@", "Public")).exists, "the code shows before the warning")
        button(tr("Show the code")).tap()
        let qr = wait(element(tr("QR code of %@", "Public")), 5, "no code")
        let text = try XCTUnwrap(OnboardingTests.decodeQR(qr.screenshot().image), "the code does not decode")
        XCTAssertTrue(text.hasPrefix("meshcore://channel/add?name=Public"), text)
        keep("mc-channel-code")
        button(tr("Done")).tap()
        if button(tr("Done")).exists { button(tr("Done")).tap() }
        back()
        // A code scanned: its channel joined, and opened.
        button(tr("New message")).tap()
        button(tr("Join a channel")).tap()
        cardButton(tr("Scan a channel’s QR code")).tap()
        wait(element("Ridge"), 5, "the scanned channel is not shown")
        button(tr("Join the channel")).tap()
        wait(app.navigationBars.firstMatch.descendants(matching: .any).matching(NSPredicate(format: "label BEGINSWITH 'Ridge'")).firstMatch,
             10, "the joined channel did not open")
        back()
        wait(row("Ridge"), 5, "no row for the joined channel")
    }

    /// The MeshCore app's export ("export app database", its schema 29): Marc both ways a week
    /// ago and a post on Public, as Android's MC-4 and QA-core's build it. `me` and `marc` are
    /// the radios' keys, hex. Returns its path.
    private func meshcoreExport(me: String, marc: String) throws -> String {
        let at = Int64(Date().timeIntervalSince1970) - 7 * 86_400
        let path = NSTemporaryDirectory() + "meshcore-test-export.db"
        try? FileManager.default.removeItem(atPath: path)
        var db: OpaquePointer?
        XCTAssertEqual(sqlite3_open(path, &db), SQLITE_OK)
        defer { sqlite3_close(db) }
        for sql in [
            #"CREATE TABLE "contact_messages" ("id" INTEGER PRIMARY KEY, "status" TEXT NOT NULL, "to" TEXT NOT NULL, "from" TEXT NOT NULL, "txt_type" INTEGER NOT NULL, "sender_timestamp" INTEGER NOT NULL, "text" TEXT NOT NULL, "timestamp" INTEGER NOT NULL, "snr" REAL NULL)"#,
            #"CREATE TABLE "channel_messages" ("id" INTEGER PRIMARY KEY, "channel_secret" BLOB NULL, "from" TEXT NULL, "txt_type" INTEGER NOT NULL, "sender_timestamp" INTEGER NOT NULL, "text" TEXT NOT NULL, "timestamp" INTEGER NOT NULL, "snr" REAL NULL)"#,
            #"CREATE TABLE "contacts" ("id" INTEGER PRIMARY KEY, "public_key" BLOB NOT NULL, "adv_name" TEXT NOT NULL, "custom_name" TEXT NULL)"#,
            #"INSERT INTO contact_messages (status, "to", "from", txt_type, sender_timestamp, text, timestamp) VALUES ('received', '\#(me)', '\#(marc)', 0, \#(at), 'Are you coming up this weekend?', \#(at * 1000))"#,
            #"INSERT INTO contact_messages (status, "to", "from", txt_type, sender_timestamp, text, timestamp) VALUES ('delivered', '\#(marc)', '\#(me)', 0, \#(at + 60), 'Saturday, with a friend.', \#((at + 60) * 1000))"#,
            // Public's secret, 8b3387e9c5cdea6ac9e5edbaa115cd72.
            #"INSERT INTO channel_messages (channel_secret, "from", txt_type, sender_timestamp, text, timestamp) VALUES (X'8B3387E9C5CDEA6AC9E5EDBAA115CD72', NULL, 0, \#(at + 120), 'Lucie: Snow above 1800 m', \#((at + 120) * 1000))"#,
            #"INSERT INTO contacts (public_key, adv_name, custom_name) VALUES (X'\#(marc)', 'Marc', NULL)"#,
        ] {
            XCTAssertEqual(sqlite3_exec(db, sql, nil, nil, nil), SQLITE_OK, sql)
        }
        return path
    }

    /// MC-4: the MeshCore app's history comes in once, older than what lichen got; a second
    /// import adds nothing. Its export is built here with this demo's keys (read in the app),
    /// and handed over with `-importFile`, the Files picker's stand-in.
    func testMC4_ImportTheMeshCoreHistory() throws {
        // The keys the export names: our radio's (Radio settings) and Marc's (his chat's info).
        launch(["-quietDemo", "YES", "-tab", "settings", "-screen", "radio"])
        reveal(meshcoreKey)
        let myKey = try XCTUnwrap(meshcoreKey.value as? String)
        tab(.messages)
        wait(row("Marc"), 60, "no message from Marc").tap()
        button(tr("MeshCore · not lichen-encrypted")).tap()
        reveal(meshcoreKey)
        let marcKey = try XCTUnwrap(meshcoreKey.value as? String)
        let path = try meshcoreExport(me: myKey, marc: marcKey)
        // The same demo again (its keys are fixed), the export to import.
        relaunch(["-quietDemo", "YES", "-importFile", path], demo: true)
        wait(row("Marc"), 60, "no message from Marc")
        tab(.settings)
        let history = app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", tr("Import from the MeshCore app"))).firstMatch
        reveal(history)
        history.tap()
        let alert = wait(app.alerts[tr("Import")], 10, "no import")
        let imported = tr("%lld direct messages and %lld channel posts imported.", 2, 1)
        XCTAssertTrue(alert.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", imported)).firstMatch.exists,
                      "not \(imported): \(alert.debugDescription)")
        keep("mc-import")
        alert.buttons["OK"].tap()
        // Again: nothing more, said so.
        history.tap()
        let again = wait(app.alerts[tr("Import")], 10, "no second import")
        let none = tr("%lld direct messages and %lld channel posts imported.", 0, 0) + " " + tr("%lld were already here.", 3)
        XCTAssertTrue(again.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", none)).firstMatch.exists,
                      "not \(none): \(again.debugDescription)")
        again.buttons["OK"].tap()
        // Marc's chat: the two older messages first, his hello still the last; each once.
        tab(.messages)
        row("Marc").tap()
        let older = wait(textOf("Are you coming up this weekend?"), 10, "his older message is not there")
        let ours = wait(textOf("Saturday, with a friend."), 5, "our older answer is not there")
        let hello = wait(textOf(marc), 5, "his hello is gone")
        XCTAssertLessThan(older.frame.minY, ours.frame.minY)
        XCTAssertLessThan(ours.frame.minY, hello.frame.minY, "the history after his hello")
        XCTAssertEqual(bubbles("Are you coming up this weekend?"), 1, "imported twice")
        keep("mc-imported")
    }
}

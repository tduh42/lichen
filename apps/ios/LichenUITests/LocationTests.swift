import XCTest

/// Location, map, Locate and SOS (docs/testing.md §8), on the demo: Camille shares her position
/// (exact, to us) about 20 s in at speed 4, Julien his (1 km, to the group) once the group's
/// keys are out; with `-sosdemo`, Julien sends an SOS without GPS, to be located by radio.
final class LocationTests: LichenUITest {
    /// A person's marker on the map: "Camille · now".
    private func marker(_ name: String) -> XCUIElement {
        app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", name + " ·")).firstMatch
    }

    /// A node's marker on the map (its name alone).
    private func nodeMarker(_ name: String) -> XCUIElement {
        app.buttons.matching(NSPredicate(format: "label == %@", name)).firstMatch
    }

    /// Flips a switch (its knob, at its trailing end).
    private func flip(_ toggle: XCUIElement) {
        toggle.coordinate(withNormalizedOffset: CGVector(dx: 0.9, dy: 0.5)).tap()
    }

    /// MP-1: our position shared with Camille (`-share Camille`, 100 m): said in her chat and
    /// on the map with its precision; stopped from the map, it ends there and in her chat.
    func testMP1_ShareMyPositionWithAContact() {
        launch(["-open", "Camille", "-share", "Camille", "-quietDemo", "YES"])
        openedCamille()
        let sharing = containing(tr("Sharing your location (%@) until %@", "100 m", "#").components(separatedBy: "#")[0])
        wait(sharing, 30, "not said in her chat")
        keep("mp-share-chat")
        tab(.map)
        let toggle = wait(app.switches[tr("Location sharing")], 10, "no location sharing")
        XCTAssertEqual(toggle.value as? String, "1", "not on")
        XCTAssertTrue(app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Camille,' AND label ENDSWITH '100 m'")).firstMatch.exists,
                      "Camille is not listed with 100 m")
        keep("mp-share-map")
        flip(toggle)
        waitValue(toggle, "0", 5)
        tab(.messages)
        wait(composer, 5, "her chat is gone")
        waitGone(sharing, 10, "still said to be shared in her chat")
    }

    /// MP-2: Camille's position (exact, "now") and Julien's (to the group, a 1 km cell) on the
    /// map, and ours.
    func testMP2_PositionsReceivedOnTheMap() {
        launch(["-tab", "map"])
        wait(marker("Camille"), 60, "no Camille on the map")
        XCTAssertTrue(nodeMarker(tr("You")).exists || marker(tr("You")).exists, "we are not on the map")
        // Once the group's keys are out: a minute on a quiet air, up to several when the
        // demo's hub is busy.
        wait(marker("Julien"), 240, "no Julien on the map")
        keep("mp-positions")
        XCTAssertTrue(marker("Julien").label.contains("1 km"), "Julien's is not a 1 km cell: \(marker("Julien").label)")
    }

    /// MP-3: the nodes layer: the repeaters and the room server where they advertise; off in
    /// Map layers, they go.
    func testMP3_NodesLayer() {
        launch(["-tab", "map", "-quietDemo", "YES"])
        wait(nodeMarker("hub"), 30, "no hub on the map")
        XCTAssertTrue(nodeMarker("Refuge board").exists, "no room server on the map")
        button(tr("Map layers")).tap()
        let layer = wait(app.buttons[tr("MeshCore nodes")], 5, "no nodes layer")
        layer.tap()
        waitGone(nodeMarker("hub"), 5, "the nodes stayed")
        XCTAssertFalse(nodeMarker("Refuge board").exists)
        button(tr("Map layers")).tap()
        wait(app.buttons[tr("MeshCore nodes")], 5).tap()
        wait(nodeMarker("hub"), 5, "the nodes did not come back")
    }

    /// MP-4: an area of the map to keep offline is offered (the download itself needs the
    /// Internet: not run here).
    func testMP4_OfflineMapsOffered() {
        launch(["-tab", "map", "-quietDemo", "YES"])
        button(tr("Map layers")).tap()
        wait(app.buttons[tr("Download this area")], 5, "no offline maps")
    }

    /// MP-5: Julien's SOS arrives (`-sosdemo`): its alert, without GPS, a beacon; ours, sent by
    /// holding the button, is on until stopped.
    func testMP5_SosReceivedSentStopped() {
        launch(["-tab", "map", "-sosdemo", "YES"])
        wait(containing(tr("SOS alert · %@", "#").components(separatedBy: "#")[0]), 90, "no SOS from Julien")
        XCTAssertTrue(element("Julien").exists, "not Julien's")
        XCTAssertTrue(containing(tr("No GPS position")).exists, "not said to be without GPS")
        keep("mp-sos-received")
        button(tr("Dismiss")).tap()
        // Ours: held 3 s.
        button(tr("SOS")).tap()
        let hold = wait(button(tr("Send an SOS")), 5, "no SOS button")
        hold.press(forDuration: 3.5)
        let stop = wait(button(tr("Stop the SOS")), 5, "the SOS did not start")
        keep("mp-sos-on")
        stop.tap()
        wait(button(tr("Send an SOS")), 5, "the SOS did not stop")
    }

    /// MP-6: Julien, without GPS, located by radio: started from his alert, the search opens on
    /// him, takes measures, and shows where he probably is, in metres or kilometres (D-13).
    func testMP6_LocateAMemberWithoutGps() {
        launch(["-tab", "map", "-sosdemo", "YES"])
        let start = wait(button(tr("Start locating")), 90, "no SOS from Julien")
        start.tap()
        wait(element(tr("Locate · %@", "Julien")), 10, "not locating Julien")
        XCTAssertTrue(button(tr("Stop locating")).exists, "no way to stop")
        let estimate = app.descendants(matching: .any).matching(NSPredicate(format: "label BEGINSWITH %@",
            tr("Map: %@ is probably within %@ of the centre", "Julien", "#").components(separatedBy: "#")[0])).firstMatch
        wait(estimate, 120, "no estimate of where he is")
        // D-13: in metres or kilometres, whatever the locale's own units (the English runs'
        // en_GB writes miles).
        let text = estimate.label
        XCTAssertNotNil(text.range(of: #"\d\s?(m|km)\b"#, options: .regularExpression), "not in m or km: \(text)")
        XCTAssertNil(text.range(of: #"\d\s?(mi|ft|yd)\b"#, options: .regularExpression), "in miles or feet: \(text)")
        keep("mp-locate")
    }
}

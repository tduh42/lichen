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
        launch(["-tab", "map", "-demoClock", "YES"])
        // Camille's about 70 s into the demo; the map keeps the app busy, and the demo falls
        // behind the wall clock: the waits go by its own clock.
        waitDemo(marker("Camille"), 150, "no Camille on the map")
        XCTAssertTrue(nodeMarker(tr("You")).exists || marker(tr("You")).exists, "we are not on the map")
        // Once the group's keys are out: about 4 minutes into the demo on most runs, 9 when its
        // hub is busy (214 to 544 s, measured in the core over five seeds).
        waitDemo(marker("Julien"), 550, "no Julien on the map")
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
        // The map redraws its markers on the main thread: seconds on a loaded machine.
        waitGone(nodeMarker("hub"), 15, "the nodes stayed")
        XCTAssertFalse(nodeMarker("Refuge board").exists)
        button(tr("Map layers")).tap()
        wait(app.buttons[tr("MeshCore nodes")], 5).tap()
        wait(nodeMarker("hub"), 15, "the nodes did not come back")
    }

    /// Any element read as `text` followed by something ("hub to crest, " then its SNR).
    private func starting(_ text: String) -> XCUIElement {
        app.descendants(matching: .any).matching(NSPredicate(format: "label BEGINSWITH %@", text)).firstMatch
    }

    /// MP-3: crest's route on the map, from its page's Show on the map: the card (to whom, one
    /// hop, via hub), the legend, each leg read ("You to hub, 7.5 dB"; "hub to crest, not
    /// measured" before a trace); closed, our shares come back. A node tapped on the map draws
    /// its own route; the layers menu has the legend.
    func testMP3_RouteOnTheMap() {
        launch(["-quietDemo", "YES"])
        tab(.nodes)
        let hubRow = app.buttons.matching(NSPredicate(format: "identifier == 'node' AND label BEGINSWITH 'hub'")).firstMatch
        waitLabel(wait(hubRow, 30, "no hub"), contains: " dB", 30)
        app.buttons.matching(NSPredicate(format: "identifier == 'node' AND label BEGINSWITH 'crest'")).firstMatch.tap()
        wait(button(tr("Show on the map")), 5, "no Show on the map").tap()
        wait(element(tr("Route to %@", "crest")), 10, "no route card")
        let card = containing(tr("%lld hops", 1))
        XCTAssertTrue(card.exists && card.label.contains(tr("via %@", "hub")), "the card: \(card.label)")
        XCTAssertTrue(containing(tr("the last signal per leg")).exists, "no legend")
        let hubLeg = starting(tr("%@ to %@, %@", tr("You"), "hub", ""))
        wait(hubLeg, 5, "the leg to the hub is not read")
        XCTAssertTrue(hubLeg.label.hasSuffix(" dB"), hubLeg.label)
        XCTAssertTrue(element(tr("%@ to %@, %@", "hub", "crest", tr("not measured"))).exists
                      || containing(tr("%@ to %@, %@", "hub", "crest", tr("not measured"))).exists, "the leg to crest is not read")
        // The route reads only its own nodes: the board, beside it, fades.
        XCTAssertEqual(nodeMarker("Refuge board").value as? String, tr("not on this route"), "the board is not faded")
        XCTAssertNotEqual(nodeMarker("hub").value as? String, tr("not on this route"), "a stop is faded")
        keep("mp-route")
        button(tr("Close the route")).tap()
        waitGone(element(tr("Route to %@", "crest")), 5, "the card stayed")
        XCTAssertNotEqual(nodeMarker("Refuge board").value as? String, tr("not on this route"), "still faded")
        wait(element(tr("Location sharing")), 5, "our shares did not come back")
        // A node tapped on the map: its route.
        nodeMarker("hub").tap()
        wait(element(tr("Route to %@", "hub")), 10, "a node tapped draws no route")
        button(tr("Close the route")).tap()
        // The legend in the layers menu.
        button(tr("Map layers")).tap()
        wait(app.buttons[tr("good")], 5, "no legend in the layers")
        XCTAssertTrue(app.buttons[tr("not measured")].exists)
    }

    /// MP-4: an area of the map to keep offline is offered (the download itself needs the
    /// Internet: not run here).
    func testMP4_OfflineMapsOffered() {
        launch(["-tab", "map", "-quietDemo", "YES"])
        button(tr("Map layers")).tap()
        wait(app.buttons[tr("Download this area")], 5, "no offline maps")
    }

    /// Chooses which nodes the map shows, in its layers menu: the choice whose label starts with
    /// `filter` ("Repeaters 2", "Heard in 2 h").
    private func mapFilter(_ filter: String, file: StaticString = #filePath, line: UInt = #line) {
        button(tr("Map layers")).tap()
        let choice = app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", tr(filter))).firstMatch
        wait(choice, 5, "no “\(tr(filter))” in the layers menu", file: file, line: line).tap()
    }

    /// Taps Dismiss on an SOS alert when one shows, without failing when the app takes it away
    /// as it is tapped (it can go and come back a moment after): a snapshot, which does not
    /// fail when nothing matches, then a tap where it was.
    private func dismissAlert() {
        guard let alert = try? button(tr("Dismiss")).snapshot() else { return }
        let f = alert.frame
        app.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: f.midX, dy: f.midY)).tap()
    }

    /// Crest's route on the map, from its page's Show on the map: the camera takes the route in
    /// (crest, 3.3 km away, is out of the map's first view, and only markers in view are read).
    private func showCrestsRoute(file: StaticString = #filePath, line: UInt = #line) {
        tab(.nodes)
        let crest = app.buttons.matching(NSPredicate(format: "identifier == 'node' AND label BEGINSWITH 'crest'")).firstMatch
        wait(crest, 30, "no crest in the Nodes tab", file: file, line: line).tap()
        wait(button(tr("Show on the map")), 5, "no Show on the map", file: file, line: line).tap()
        wait(element(tr("Route to %@", "crest")), 10, "no route to crest", file: file, line: line)
    }

    /// MP-3: the nodes layer's filters, in the layers menu: the Nodes tab's chips with their
    /// counts. Repeaters leaves only the repeaters; the Nodes tab keeps its own (everyone), and
    /// the map's is kept on the way back; a route's stops stay whatever it leaves (crest's route
    /// under Rooms); Heard in 2 h keeps the nodes heard lately; All brings every node back.
    func testMP3_NodeFiltersOnTheMap() {
        launch(["-tab", "map", "-quietDemo", "YES"])
        for name in ["hub", "Refuge board"] { wait(nodeMarker(name), 30, "no \(name) on the map") }
        // The choices, with how many each keeps.
        button(tr("Map layers")).tap()
        let counted = NSPredicate(format: "label MATCHES %@", NSRegularExpression.escapedPattern(for: tr("Repeaters")) + " [0-9]+")
        let repeaters = app.buttons.matching(counted).firstMatch
        wait(repeaters, 5, "no Repeaters with its count in the layers menu")
        XCTAssertTrue(app.buttons[tr("Heard in 2 h")].exists, "no Heard in 2 h")
        keep("mp-node-filters")
        repeaters.tap()
        waitGone(nodeMarker("Refuge board"), 15, "the room server stayed under Repeaters")
        XCTAssertTrue(nodeMarker("hub").exists, "a repeater left")
        // The Nodes tab's own filter: everyone still there. Crest's route from its page: under
        // Repeaters, the board stays away; under Rooms, the board comes and the route's stops
        // (crest, hub) stay.
        showCrestsRoute()
        wait(nodeMarker("crest"), 10, "no crest on its route")
        XCTAssertFalse(nodeMarker("Refuge board").exists, "the map's filter was not kept")
        mapFilter("Rooms")
        wait(nodeMarker("Refuge board"), 15, "no room server under Rooms")
        XCTAssertTrue(nodeMarker("crest").exists && nodeMarker("hub").exists, "a stop of the route left")
        keep("mp-node-filters-route")
        button(tr("Close the route")).tap()
        waitGone(nodeMarker("crest"), 15, "crest stayed under Rooms without its route")
        XCTAssertFalse(nodeMarker("hub").exists, "the hub stayed under Rooms without the route")
        // Heard in 2 h: the nodes heard lately (every placed node of the demo), then All.
        mapFilter("Heard in 2 h")
        wait(nodeMarker("hub"), 15, "the hub, heard lately, left")
        mapFilter("All")
        for name in ["hub", "crest", "Refuge board"] { wait(nodeMarker(name), 15, "\(name) did not come back") }
    }

    /// MP-3: an SOS is never faded by a route (`-sosdemo2`: Camille's SOS, with her position,
    /// about 240 s into the demo, after Julien's): with crest's route shown, her marker reads at
    /// full strength while the Refuge board, beside the route, is faded.
    func testMP3_AnSosIsNeverFaded() {
        launch(["-tab", "map", "-sosdemo2", "YES"])
        // Julien's alert first, then Camille's: each dismissed (one can come back a moment after).
        let camille = app.buttons.matching(NSPredicate(format: "label == 'SOS · Camille'")).firstMatch
        let end = Date().addingTimeInterval(240)
        while Date() < end && !(camille.exists && camille.isHittable) {
            dismissAlert()
            pause(2)
        }
        wait(camille, 5, "no SOS from Camille on the map")
        showCrestsRoute()
        // Her alert can come back over the map.
        for _ in 0..<3 where button(tr("Dismiss")).exists {
            dismissAlert()
            pause(1)
        }
        wait(nodeMarker("Refuge board"), 10, "no board beside the route")
        XCTAssertEqual(nodeMarker("Refuge board").value as? String, tr("not on this route"), "the board is not faded")
        wait(camille, 10, "Camille's SOS is not in the route's view")
        XCTAssertNotEqual(camille.value as? String, tr("not on this route"), "Camille's SOS is faded")
        keep("mp-sos-not-faded")
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

    /// MP-5: in Locate, the Map's other mode, an SOS alert still comes over everything, and the
    /// SOS button stays at the top: ours starts and stops from there.
    func testMP5_SosFromLocate() {
        launch(["-tab", "locate", "-sosdemo", "YES"])
        wait(button(tr("Dismiss")), 90, "no SOS alert over Locate").tap()
        XCTAssertTrue(button(tr("Positions")).exists, "not on the Map's Locate")
        // Julien's alert can come back a moment after it was dismissed and cover the map; a tap
        // while it goes away can be lost: dismissed again, then SOS again, until its sheet shows.
        let send = button(tr("Send an SOS"))
        for _ in 0..<5 where !send.exists {
            if button(tr("Dismiss")).exists {
                tapIfThere(button(tr("Dismiss")))
                pause(1)
            }
            button(tr("SOS")).tap()
            _ = send.waitForExistence(timeout: 3)
        }
        let hold = wait(send, 5, "no SOS from Locate")
        hold.press(forDuration: 3.5)
        // The SOS starts on the main thread, behind the map: seconds on a loaded machine.
        let stop = wait(button(tr("Stop the SOS")), 15, "the SOS did not start")
        keep("mp-sos-from-locate")
        stop.tap()
        wait(button(tr("Send an SOS")), 15, "the SOS did not stop")
    }

    /// MP-6: Locate is a mode of the Map (Positions | Locate at the top): it shows the search
    /// for Julien, whose SOS came without GPS; Positions brings the map back.
    func testMP6_LocateIsAModeOfTheMap() {
        launch(["-tab", "map", "-sosdemo", "YES"])
        wait(button(tr("Dismiss")), 90, "no SOS from Julien").tap()
        // The Map tab counts the SOS cases still active (the Locate tab did).
        let badge = app.tabBars.buttons.element(boundBy: Tab.map.rawValue).value as? String ?? ""
        XCTAssertTrue(badge.contains("1"), "the Map tab's badge: \(badge)")
        locateMode()
        wait(element(tr("Locate · %@", "Julien")), 10, "Locate does not show Julien")
        keep("mp-locate-mode")
        button(tr("Positions")).tap()
        wait(button(tr("Show everyone")), 5, "the map did not come back")
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

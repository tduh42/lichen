import MapLibre
import SwiftUI

/// Map styles from OpenFreeMap: no key, no account (ADR-0011). `auto` follows the theme.
enum MapStyle: String, CaseIterable, Identifiable {
    case auto, dark, liberty, positron, bright

    var id: String { rawValue }

    /// The style actually drawn.
    func resolved(dark: Bool) -> MapStyle {
        self == .auto ? (dark ? .dark : .liberty) : self
    }

    var url: URL { URL(string: "https://tiles.openfreemap.org/styles/\(rawValue)")! }

    var label: LocalizedStringKey {
        switch self {
        case .auto: "Follow the theme"
        case .dark: "Dark"
        case .liberty: "Outdoor"
        case .positron: "Light"
        case .bright: "Bright"
        }
    }
}

/// Something drawn on the map: us, or someone's position with its circle of uncertainty, or
/// a MeshCore node at the position it advertises (protocol.md §16.1).
struct MapItem: Equatable, Identifiable {
    /// `node`: a mesh node (ADR-0041 to ADR-0044); `place`: coordinates from a message (ADR-0034).
    enum Kind: Equatable { case me, peer, sos, node, place }

    let id: String
    let kind: Kind
    let lat: Double
    let lon: Double
    /// Radius of the circle, metres (none below 25 m).
    let radius: Double
    let initials: String
    let label: String
    /// Resolved for `dark`.
    let color: UIColor
    /// Drawn for the dark theme.
    let dark: Bool
    /// A node's symbol (SF Symbols).
    var symbol: String? = nil
    /// A node heard directly: a thin ring in its rating's colour (ADR-0049).
    var ring: UIColor? = nil
    /// Not a stop of the route shown: faded, so that the route reads only its own nodes.
    var dimmed = false

    var coordinate: CLLocationCoordinate2D { .init(latitude: lat, longitude: lon) }
}

/// A search estimate on the map: the heat grid as an image over its bounds, and the 68 % and
/// 95 % ellipses. `version` tells a new estimate.
struct MapOverlay: Equatable {
    let version: Int
    let heat: UIImage?
    let north: Double
    let south: Double
    let west: Double
    let east: Double
    let ellipse68: [CLLocationCoordinate2D]
    let ellipse95: [CLLocationCoordinate2D]

    static func == (a: MapOverlay, b: MapOverlay) -> Bool { a.version == b.version }
}

/// A node's route on the map (§16.3): a line per leg between the stops that have a position,
/// in its rating's colour, dashed where nothing measured it; its SNR on a chip near its middle.
struct MapRoute: Equatable {
    struct Line: Equatable {
        let fromLat: Double
        let fromLon: Double
        let toLat: Double
        let toLon: Double
        let color: UIColor
        let dashed: Bool
        /// "7.5 dB", "not measured"; several legs' joined across stops without a position.
        let chip: String
        let chipColor: UIColor
        /// "hub to crest, −3.0 dB", as VoiceOver reads the leg.
        let said: String
    }

    /// The node's key (hex).
    let key: String
    let lines: [Line]
}

/// MapLibre Native in SwiftUI: items as annotation views, circles as polygons.
struct MapLibreView: UIViewRepresentable {
    let items: [MapItem]
    let style: MapStyle
    /// Bumped to fit the map around `items`.
    let recenter: Int
    /// Reports the visible area (for offline downloads).
    let onCamera: (MLNCoordinateBounds, Double) -> Void
    /// A search estimate to draw.
    var overlay: MapOverlay?
    /// Margins kept free when fitting (the full-screen map has buttons and labels on top).
    var insets = UIEdgeInsets(top: 140, left: 95, bottom: 70, right: 95)
    /// A place to centre on, once per `version` (after any `recenter`).
    var focus: MapPin?
    /// A node's route to draw, the camera taking it in once per node.
    var route: MapRoute?
    /// An item tapped (its id).
    var onSelect: ((String) -> Void)?
    /// The route's legs whose chip finds no room on the map (by index): said in the card.
    var onHiddenChips: (([Int]) -> Void)?

    func makeUIView(context: Context) -> MapContainer {
        let map = MLNMapView(frame: .zero, styleURL: style.url)
        map.delegate = context.coordinator
        map.compassViewPosition = .bottomLeft
        map.compassViewMargins = CGPoint(x: 16, y: 16)
        // OpenStreetMap / OpenFreeMap attribution, out of the way of the top buttons.
        map.attributionButtonPosition = .bottomRight
        map.attributionButtonMargins = CGPoint(x: 8, y: 8)
        map.logoView.isHidden = true
        // VoiceOver names MapLibre's own controls.
        map.attributionButton.accessibilityLabel = String(localized: "Map credits")
        map.compassView.accessibilityLabel = String(localized: "Compass: tap to face north")
        map.setCenter(CLLocationCoordinate2D(latitude: 45.0, longitude: 5.5), zoomLevel: 9, animated: false)
        context.coordinator.style = style
        // The route's chips over the map, beside it: VoiceOver reads them (the map lists only
        // its own annotations).
        return MapContainer(map: map, chips: context.coordinator.chips)
    }

    func updateUIView(_ container: MapContainer, context: Context) {
        let map = container.map
        let c = context.coordinator
        c.parent = self
        if c.style != style {
            c.style = style
            map.styleURL = style.url
        }
        c.apply(items, to: map)
        c.applyOverlay(overlay, to: map)
        c.applyRoute(route, to: map)
        if c.recenter != recenter {
            c.recenter = recenter
            fit(map)
        }
        if let focus, c.focus != focus.version {
            c.focus = focus.version
            map.setCenter(CLLocationCoordinate2D(latitude: focus.lat, longitude: focus.lon),
                          zoomLevel: max(map.zoomLevel, 14), animated: true)
        }
    }

    /// Web Mercator ends here: a camera asked to show beyond it has no answer.
    static let latitudeMax = 85.0

    private func fit(_ map: MLNMapView) {
        // People first: nodes count only when nobody else is on the map.
        let people = self.items.filter { $0.kind != .node }
        let items = people.isEmpty ? self.items : people
        guard let first = items.first else { return }
        if items.count == 1 && first.radius < 500 && overlay == nil {
            map.setCenter(first.coordinate, zoomLevel: max(map.zoomLevel, 13), animated: true)
            return
        }
        // Every marker with its whole circle of uncertainty.
        var sw = first.coordinate, ne = first.coordinate
        for i in items {
            let dlat = i.radius / 111_320
            let dlon = i.radius / (111_320 * max(0.01, cos(i.lat * .pi / 180)))
            sw.latitude = min(sw.latitude, i.lat - dlat)
            sw.longitude = min(sw.longitude, i.lon - dlon)
            ne.latitude = max(ne.latitude, i.lat + dlat)
            ne.longitude = max(ne.longitude, i.lon + dlon)
        }
        // Positions come from other people: near a pole, a circle reaches past what the map
        // projects (review of 2026-11-15, SR-37).
        sw.latitude = max(sw.latitude, -Self.latitudeMax)
        ne.latitude = min(ne.latitude, Self.latitudeMax)
        // And the search estimate's 95 % ellipse.
        for p in overlay?.ellipse95 ?? [] {
            sw.latitude = min(sw.latitude, p.latitude)
            sw.longitude = min(sw.longitude, p.longitude)
            ne.latitude = max(ne.latitude, p.latitude)
            ne.longitude = max(ne.longitude, p.longitude)
        }
        let bounds = MLNCoordinateBounds(sw: sw, ne: ne)
        // Below the top buttons; the sharing panel is outside the map's frame already.
        // Labels are up to ~180 pt wide, centred on their marker.
        let camera = map.cameraThatFitsCoordinateBounds(bounds, edgePadding: insets)
        // Never closer than street level: two people side by side stay in context.
        let maxAltitude = MLNAltitudeForZoomLevel(15, camera.pitch, bounds.sw.latitude, map.frame.size)
        camera.altitude = max(camera.altitude, maxAltitude)
        map.setCamera(camera, withDuration: 0.6, animationTimingFunction: nil)
    }

    func makeCoordinator() -> Coordinator { Coordinator(parent: self) }

    final class Coordinator: NSObject, MLNMapViewDelegate {
        var parent: MapLibreView
        var style: MapStyle?
        var recenter = 0
        var focus: Int?
        private var annotations: [String: ItemAnnotation] = [:]
        private var circles: [String: (MLNPolygon, Double, Double, Double)] = [:]
        private var circleColors: [ObjectIdentifier: UIColor] = [:]
        private var overlayVersion: Int?
        private var ellipses: [MLNPolyline] = []
        private var ellipseWidths: [ObjectIdentifier: CGFloat] = [:]
        private static let heatId = "lichen-search-heat"
        private var drawnRoute: MapRoute?
        private var routeLayers = 0
        private var fittedRoute: String?
        let chips = ChipLayer()
        private var hiddenChips: [Int] = []

        init(parent: MapLibreView) {
            self.parent = parent
        }

        @MainActor
        func apply(_ items: [MapItem], to map: MLNMapView) {
            let ids = Set(items.map(\.id))
            for (id, a) in annotations where !ids.contains(id) {
                map.removeAnnotation(a)
                annotations[id] = nil
            }
            for (id, c) in circles where !ids.contains(id) {
                removeCircle(id, c.0, from: map)
            }
            for item in items {
                if let a = annotations[item.id] {
                    if a.item != item {
                        let redraw = a.item.label != item.label || a.item.kind != item.kind
                            || a.item.initials != item.initials || a.item.dark != item.dark || a.item.ring != item.ring
                        a.item = item
                        a.coordinate = item.coordinate
                        if redraw { a.image = Self.render(item) }
                        // Faded or back, without drawing it again.
                        (map.view(for: a) as? ItemAnnotationView)?.show(a)
                    }
                } else {
                    let a = ItemAnnotation(item: item, image: Self.render(item))
                    annotations[item.id] = a
                    map.addAnnotation(a)
                }
                circle(for: item, on: map)
            }
        }

        @MainActor
        private func circle(for item: MapItem, on map: MLNMapView) {
            let old = circles[item.id]
            guard item.radius >= 25 else {
                if let old { removeCircle(item.id, old.0, from: map) }
                return
            }
            if let old, old.1 == item.lat, old.2 == item.lon, old.3 == item.radius { return }
            if let old { removeCircle(item.id, old.0, from: map) }
            var coords = (0..<64).map { i -> CLLocationCoordinate2D in
                let a = Double(i) / 64 * 2 * .pi
                let dlat = item.radius * cos(a) / 111_320
                let dlon = item.radius * sin(a) / (111_320 * max(0.01, cos(item.lat * .pi / 180)))
                return CLLocationCoordinate2D(latitude: min(max(item.lat + dlat, -90), 90), longitude: item.lon + dlon)
            }
            let polygon = MLNPolygon(coordinates: &coords, count: UInt(coords.count))
            circleColors[ObjectIdentifier(polygon)] = item.color
            circles[item.id] = (polygon, item.lat, item.lon, item.radius)
            map.addAnnotation(polygon)
        }

        @MainActor
        private func removeCircle(_ id: String, _ polygon: MLNPolygon, from map: MLNMapView) {
            map.removeAnnotation(polygon)
            circleColors[ObjectIdentifier(polygon)] = nil
            circles[id] = nil
        }

        /// Draws `overlay` (again after a style change, which drops its layers).
        @MainActor
        func applyOverlay(_ overlay: MapOverlay?, to map: MLNMapView, force: Bool = false) {
            guard force || overlay?.version != overlayVersion else { return }
            guard let style = map.style else { return }
            overlayVersion = overlay?.version
            if let l = style.layer(withIdentifier: Self.heatId) { style.removeLayer(l) }
            if let s = style.source(withIdentifier: Self.heatId) { style.removeSource(s) }
            map.removeAnnotations(ellipses)
            for e in ellipses { ellipseWidths[ObjectIdentifier(e)] = nil; circleColors[ObjectIdentifier(e)] = nil }
            ellipses = []
            guard let overlay else { return }
            if let heat = overlay.heat {
                let quad = MLNCoordinateQuad(
                    topLeft: CLLocationCoordinate2D(latitude: overlay.north, longitude: overlay.west),
                    bottomLeft: CLLocationCoordinate2D(latitude: overlay.south, longitude: overlay.west),
                    bottomRight: CLLocationCoordinate2D(latitude: overlay.south, longitude: overlay.east),
                    topRight: CLLocationCoordinate2D(latitude: overlay.north, longitude: overlay.east)
                )
                let source = MLNImageSource(identifier: Self.heatId, coordinateQuad: quad, image: heat)
                style.addSource(source)
                let layer = MLNRasterStyleLayer(identifier: Self.heatId, source: source)
                layer.rasterOpacity = NSExpression(forConstantValue: 0.8)
                layer.rasterFadeDuration = NSExpression(forConstantValue: 0)
                style.addLayer(layer)
            }
            let red = UIColor(red: 0.86, green: 0.15, blue: 0.15, alpha: 1)
            for (coords, width) in [(overlay.ellipse95, CGFloat(1.5)), (overlay.ellipse68, CGFloat(2.5))] where !coords.isEmpty {
                var c = coords
                let line = MLNPolyline(coordinates: &c, count: UInt(c.count))
                circleColors[ObjectIdentifier(line)] = red
                ellipseWidths[ObjectIdentifier(line)] = width
                ellipses.append(line)
                map.addAnnotation(line)
            }
        }

        func mapView(_ mapView: MLNMapView, didFinishLoading style: MLNStyle) {
            MainActor.assumeIsolated {
                applyOverlay(parent.overlay, to: mapView, force: true)
                applyRoute(parent.route, to: mapView, force: true)
            }
        }

        /// Draws `route` (again after a style change, which drops its layers): a line layer per
        /// leg, dashed where nothing measured it; the camera takes a new route in.
        @MainActor
        func applyRoute(_ route: MapRoute?, to map: MLNMapView, force: Bool = false) {
            guard force || route != drawnRoute else { return }
            guard let style = map.style else { return }
            drawnRoute = route
            for i in 0..<routeLayers {
                if let l = style.layer(withIdentifier: "lichen-route-\(i)") { style.removeLayer(l) }
                if let s = style.source(withIdentifier: "lichen-route-\(i)") { style.removeSource(s) }
            }
            routeLayers = 0
            chips.show(route?.lines ?? [])
            guard let route else {
                fittedRoute = nil
                layoutChips(map)
                return
            }
            for (i, line) in route.lines.enumerated() {
                var coords = [CLLocationCoordinate2D(latitude: line.fromLat, longitude: line.fromLon),
                              CLLocationCoordinate2D(latitude: line.toLat, longitude: line.toLon)]
                let feature = MLNPolylineFeature(coordinates: &coords, count: 2)
                let source = MLNShapeSource(identifier: "lichen-route-\(i)", shape: feature)
                style.addSource(source)
                let layer = MLNLineStyleLayer(identifier: "lichen-route-\(i)", source: source)
                layer.lineColor = NSExpression(forConstantValue: line.color)
                layer.lineWidth = NSExpression(forConstantValue: 4)
                layer.lineCap = NSExpression(forConstantValue: line.dashed ? "butt" : "round")
                if line.dashed { layer.lineDashPattern = NSExpression(forConstantValue: [2, 1.5]) }
                style.addLayer(layer)
                routeLayers += 1
            }
            // A new route: the camera takes all its stops in.
            if fittedRoute != route.key, !route.lines.isEmpty {
                fittedRoute = route.key
                var sw = CLLocationCoordinate2D(latitude: 90, longitude: 180)
                var ne = CLLocationCoordinate2D(latitude: -90, longitude: -180)
                for l in route.lines {
                    for (lat, lon) in [(l.fromLat, l.fromLon), (l.toLat, l.toLon)] {
                        sw.latitude = min(sw.latitude, lat); sw.longitude = min(sw.longitude, lon)
                        ne.latitude = max(ne.latitude, lat); ne.longitude = max(ne.longitude, lon)
                    }
                }
                let camera = map.cameraThatFitsCoordinateBounds(MLNCoordinateBounds(sw: sw, ne: ne),
                                                                edgePadding: UIEdgeInsets(top: 160, left: 70, bottom: 90, right: 70))
                map.setCamera(camera, withDuration: 0.6, animationTimingFunction: nil)
            }
            layoutChips(map)
        }

        /// Puts each leg's chip along it, as near its middle as is free of the markers and their
        /// labels and of the other chips; a leg with no free spot gets none on the map (its card
        /// says it).
        @MainActor
        func layoutChips(_ map: MLNMapView) {
            guard let route = drawnRoute else {
                report([])
                return
            }
            // The markers with their labels, in the map's coordinates.
            var taken: [CGRect] = annotations.values.compactMap { a in
                map.view(for: a).flatMap { $0.isHidden ? nil : $0.convert($0.bounds, to: map) }
            }
            var hidden: [Int] = []
            for (i, line) in route.lines.enumerated() {
                let a = map.convert(CLLocationCoordinate2D(latitude: line.fromLat, longitude: line.fromLon), toPointTo: map)
                let b = map.convert(CLLocationCoordinate2D(latitude: line.toLat, longitude: line.toLon), toPointTo: map)
                let size = chips.size(of: i)
                let spot = [0.5, 0.4, 0.6, 0.3, 0.7, 0.2, 0.8].lazy.map { t -> CGRect in
                    let p = CGPoint(x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t)
                    return CGRect(x: p.x - size.width / 2, y: p.y - size.height / 2, width: size.width, height: size.height)
                }.first { r in map.bounds.contains(r) && !taken.contains { $0.insetBy(dx: -2, dy: -2).intersects(r) } }
                if let spot {
                    taken.append(spot)
                    chips.place(i, at: spot)
                } else {
                    chips.place(i, at: nil)
                    hidden.append(i)
                }
            }
            report(hidden)
        }

        private func report(_ hidden: [Int]) {
            guard hidden != hiddenChips else { return }
            hiddenChips = hidden
            let tell = parent.onHiddenChips
            DispatchQueue.main.async { tell?(hidden) }
        }

        func mapViewRegionIsChanging(_ mapView: MLNMapView) {
            MainActor.assumeIsolated { layoutChips(mapView) }
        }

        func mapView(_ mapView: MLNMapView, didSelect annotation: MLNAnnotation) {
            MainActor.assumeIsolated {
                mapView.deselectAnnotation(annotation, animated: false)
                if let a = annotation as? ItemAnnotation { parent.onSelect?(a.item.id) }
            }
        }

        func mapView(_ mapView: MLNMapView, lineWidthForPolylineAnnotation annotation: MLNPolyline) -> CGFloat {
            ellipseWidths[ObjectIdentifier(annotation)] ?? 2
        }

        @MainActor
        static func render(_ item: MapItem) -> UIImage {
            let r = ImageRenderer(content: MarkerBadge(item: item))
            r.scale = UIScreen.main.scale
            return r.uiImage ?? UIImage()
        }

        func mapView(_ mapView: MLNMapView, viewFor annotation: MLNAnnotation) -> MLNAnnotationView? {
            guard let a = annotation as? ItemAnnotation else { return nil }
            return MainActor.assumeIsolated {
                let v = mapView.dequeueReusableAnnotationView(withIdentifier: "item") as? ItemAnnotationView
                    ?? ItemAnnotationView(reuseIdentifier: "item")
                v.show(a)
                return v
            }
        }

        func mapView(_ mapView: MLNMapView, annotationCanShowCallout annotation: MLNAnnotation) -> Bool { false }

        func mapView(_ mapView: MLNMapView, fillColorForPolygonAnnotation annotation: MLNPolygon) -> UIColor {
            (circleColors[ObjectIdentifier(annotation)] ?? .systemBlue).withAlphaComponent(0.12)
        }

        func mapView(_ mapView: MLNMapView, strokeColorForShapeAnnotation annotation: MLNShape) -> UIColor {
            circleColors[ObjectIdentifier(annotation)] ?? .systemBlue
        }

        func mapView(_ mapView: MLNMapView, alphaForShapeAnnotation annotation: MLNShape) -> CGFloat { 1 }

        func mapView(_ mapView: MLNMapView, regionDidChangeAnimated animated: Bool) {
            let bounds = mapView.visibleCoordinateBounds
            let zoom = mapView.zoomLevel
            MainActor.assumeIsolated {
                parent.onCamera(bounds, zoom)
                layoutChips(mapView)
            }
        }
    }
}

final class ItemAnnotation: NSObject, MLNAnnotation {
    var item: MapItem
    var image: UIImage
    @objc dynamic var coordinate: CLLocationCoordinate2D

    init(item: MapItem, image: UIImage) {
        self.item = item
        self.image = image
        coordinate = item.coordinate
    }
}

/// The rendered badge; the annotation's coordinate is the centre of the avatar (or dot).
final class ItemAnnotationView: MLNAnnotationView {
    private let imageView = UIImageView()

    override init(reuseIdentifier: String?) {
        super.init(reuseIdentifier: reuseIdentifier)
        addSubview(imageView)
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    func show(_ a: ItemAnnotation) {
        let size = a.image.size
        imageView.image = a.image
        // A marker reads as its label ("Camille, 100 m"): its image is only drawn.
        isAccessibilityElement = true
        accessibilityLabel = a.item.kind == .me ? String(localized: "You")
            : a.item.label.isEmpty ? a.item.initials : a.item.label
        // Not a stop of the route shown: faded, and said.
        alpha = a.item.dimmed ? 0.35 : 1
        accessibilityValue = a.item.dimmed ? String(localized: "not on this route") : nil
        imageView.frame = CGRect(origin: .zero, size: size)
        frame.size = size
        let anchor = MarkerBadge.padding(a.item) + (a.item.kind == .me ? MarkerBadge.dot / 2 : MarkerBadge.avatar / 2)
        centerOffset = CGVector(dx: 0, dy: size.height / 2 - anchor)
        layer.zPosition = switch a.item.kind {
        case .sos: 3
        case .place: 2
        case .me: 1
        case .peer: 0
        case .node: -1
        }
    }
}

/// An avatar (or our blue dot) with its label, as in the mockup.
struct MarkerBadge: View {
    static let avatar: CGFloat = 32
    static let dot: CGFloat = 22
    let item: MapItem

    /// Room around the marker: its ring, when it has one.
    static func padding(_ item: MapItem) -> CGFloat { item.ring == nil ? 2 : 6 }

    var body: some View {
        VStack(spacing: 4) {
            switch item.kind {
            case .me:
                Circle()
                    .fill(Color(item.color))
                    .frame(width: Self.dot, height: Self.dot)
                    .overlay(Circle().stroke(Color(hex: item.dark ? 0x0E0E0D : 0xFFFFFF), lineWidth: 3))
            case .node:
                Image(systemName: item.symbol ?? "antenna.radiowaves.left.and.right")
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(Color(hex: 0x141413))
                    .frame(width: Self.avatar, height: Self.avatar)
                    .background(Color(item.color), in: RoundedRectangle(cornerRadius: 8))
                    .overlay(RoundedRectangle(cornerRadius: 8).stroke(Color(hex: item.dark ? 0x0E0E0D : 0xFFFFFF), lineWidth: 2))
                    // Heard directly: a thin ring in its rating's colour, round for a person.
                    .overlay {
                        if let ring = item.ring {
                            RoundedRectangle(cornerRadius: 11).stroke(Color(ring), lineWidth: 2.5).padding(-4)
                        }
                    }
            case .place:
                Image(systemName: "mappin")
                    .font(.system(size: 16, weight: .bold))
                    .foregroundStyle(.white)
                    .frame(width: Self.avatar, height: Self.avatar)
                    .background(Color(item.color), in: Circle())
                    .overlay(Circle().stroke(Color(hex: item.dark ? 0x0E0E0D : 0xFFFFFF), lineWidth: 2))
            case .peer, .sos:
                Text(item.initials)
                    .font(.system(.caption, weight: .semibold))
                    .foregroundStyle(Color(hex: 0x141413))
                    .frame(width: Self.avatar, height: Self.avatar)
                    .background(Color(item.color), in: Circle())
                    .overlay(Circle().stroke(Color(hex: item.dark ? 0x0E0E0D : 0xFFFFFF), lineWidth: 2))
            }
            if !item.label.isEmpty {
                Text(item.label)
                    .font(.system(.caption2, weight: .semibold))
                    .foregroundStyle(item.kind == .sos ? .white : Color(hex: item.dark ? 0xF4F4F0 : 0x141413))
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(
                        item.kind == .sos ? Color(hex: 0xDC2626) : Color(hex: item.dark ? 0x1A1A18 : 0xFFFFFF),
                        in: RoundedRectangle(cornerRadius: 6)
                    )
                    .fixedSize()
            }
        }
        .padding(Self.padding(item))
    }
}

/// The map with the route's chips over it, in the same coordinates.
final class MapContainer: UIView {
    let map: MLNMapView

    init(map: MLNMapView, chips: ChipLayer) {
        self.map = map
        super.init(frame: .zero)
        for v in [map, chips] as [UIView] {
            v.frame = bounds
            v.autoresizingMask = [.flexibleWidth, .flexibleHeight]
            addSubview(v)
        }
    }

    required init?(coder: NSCoder) { fatalError("not used") }
}

/// The route's SNR chips over the map (`MapRoute`): placed by the map's coordinator, each read
/// as its leg ("hub to crest, −3.0 dB").
final class ChipLayer: UIView {
    private var labels: [UILabel] = []

    override init(frame: CGRect) {
        super.init(frame: frame)
        isUserInteractionEnabled = false
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    func show(_ lines: [MapRoute.Line]) {
        labels.forEach { $0.removeFromSuperview() }
        labels = lines.map { line in
            let l = PaddedLabel()
            l.text = line.chip
            // Tabular digits, growing with the text size.
            l.font = UIFontMetrics(forTextStyle: .caption1).scaledFont(for: .monospacedDigitSystemFont(ofSize: 12, weight: .bold))
            l.adjustsFontForContentSizeCategory = true
            l.textColor = line.chipColor
            l.backgroundColor = UIColor { $0.userInterfaceStyle == .dark ? UIColor(Theme.s1) : .white }
            l.layer.cornerRadius = 8
            l.layer.masksToBounds = true
            l.textAlignment = .center
            l.isAccessibilityElement = true
            l.accessibilityLabel = line.said
            l.isHidden = true
            addSubview(l)
            return l
        }
    }

    func size(of i: Int) -> CGSize {
        guard i < labels.count else { return .zero }
        let s = labels[i].intrinsicContentSize
        return CGSize(width: ceil(s.width), height: ceil(s.height))
    }

    func place(_ i: Int, at rect: CGRect?) {
        guard i < labels.count else { return }
        if let rect { labels[i].frame = rect }
        labels[i].isHidden = rect == nil
    }
}

/// A label with room around its text (a chip).
final class PaddedLabel: UILabel {
    private let inset = UIEdgeInsets(top: 3, left: 7, bottom: 3, right: 7)

    override var intrinsicContentSize: CGSize {
        let s = super.intrinsicContentSize
        return CGSize(width: s.width + inset.left + inset.right, height: s.height + inset.top + inset.bottom)
    }

    override func drawText(in rect: CGRect) {
        super.drawText(in: rect.inset(by: inset))
    }
}

/// With Tor on, map tiles would go to OpenFreeMap directly, outside Tor, and show the area
/// being looked at: the map waits until allowed, for this run of the app (review of
/// 2026-11-15, SR-36). Positions themselves never leave the phone this way.
struct MapGate<Content: View>: View {
    @Environment(AppModel.self) private var model
    @ViewBuilder var content: () -> Content

    var body: some View {
        if model.torEnabled && !model.mapWithTorAllowed {
            VStack(spacing: 12) {
                Image(systemName: "map").font(.largeTitle).foregroundStyle(Theme.tx2)
                Text("The map waits while Tor is on")
                    .font(.system(.headline))
                Text("Its tiles come from OpenFreeMap directly, outside Tor: they would show the area you look at.")
                    .font(.footnote)
                    .foregroundStyle(Theme.tx2)
                    .multilineTextAlignment(.center)
                Button("Load the map this time") { model.mapWithTorAllowed = true }
                    .buttonStyle(.bordered)
            }
            .padding(24)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Theme.bg)
        } else {
            content()
        }
    }
}


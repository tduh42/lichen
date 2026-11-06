import MapLibre
import SwiftUI

/// Map styles from OpenFreeMap: no key, no account (ADR-0011).
enum MapStyle: String, CaseIterable, Identifiable {
    case liberty, positron, bright

    var id: String { rawValue }
    var url: URL { URL(string: "https://tiles.openfreemap.org/styles/\(rawValue)")! }

    var label: LocalizedStringKey {
        switch self {
        case .liberty: "Outdoor"
        case .positron: "Light"
        case .bright: "Bright"
        }
    }
}

/// Something drawn on the map: us, or someone's position with its circle of uncertainty.
struct MapItem: Equatable, Identifiable {
    enum Kind: Equatable { case me, peer, sos }

    let id: String
    let kind: Kind
    let lat: Double
    let lon: Double
    /// Radius of the circle, metres (none below 25 m).
    let radius: Double
    let initials: String
    let label: String
    let color: UIColor

    var coordinate: CLLocationCoordinate2D { .init(latitude: lat, longitude: lon) }
}

/// MapLibre Native in SwiftUI: items as annotation views, circles as polygons.
struct MapLibreView: UIViewRepresentable {
    let items: [MapItem]
    let style: MapStyle
    /// Bumped to fit the map around `items`.
    let recenter: Int
    /// Reports the visible area (for offline downloads).
    let onCamera: (MLNCoordinateBounds, Double) -> Void

    func makeUIView(context: Context) -> MLNMapView {
        let map = MLNMapView(frame: .zero, styleURL: style.url)
        map.delegate = context.coordinator
        map.compassViewPosition = .bottomLeft
        map.compassViewMargins = CGPoint(x: 16, y: 16)
        // OpenStreetMap / OpenFreeMap attribution, out of the way of the top buttons.
        map.attributionButtonPosition = .bottomRight
        map.attributionButtonMargins = CGPoint(x: 8, y: 8)
        map.logoView.isHidden = true
        map.setCenter(CLLocationCoordinate2D(latitude: 45.0, longitude: 5.5), zoomLevel: 9, animated: false)
        context.coordinator.style = style
        return map
    }

    func updateUIView(_ map: MLNMapView, context: Context) {
        let c = context.coordinator
        c.parent = self
        if c.style != style {
            c.style = style
            map.styleURL = style.url
        }
        c.apply(items, to: map)
        if c.recenter != recenter {
            c.recenter = recenter
            fit(map)
        }
    }

    private func fit(_ map: MLNMapView) {
        guard let first = items.first else { return }
        if items.count == 1 && first.radius < 500 {
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
        let bounds = MLNCoordinateBounds(sw: sw, ne: ne)
        // Below the top buttons; the sharing panel is outside the map's frame already.
        // Labels are up to ~180 pt wide, centred on their marker.
        let insets = UIEdgeInsets(top: 140, left: 95, bottom: 70, right: 95)
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
        private var annotations: [String: ItemAnnotation] = [:]
        private var circles: [String: (MLNPolygon, Double, Double, Double)] = [:]
        private var circleColors: [ObjectIdentifier: UIColor] = [:]

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
                            || a.item.initials != item.initials
                        a.item = item
                        a.coordinate = item.coordinate
                        if redraw {
                            a.image = Self.render(item)
                            (map.view(for: a) as? ItemAnnotationView)?.show(a)
                        }
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
                let dlon = item.radius * sin(a) / (111_320 * cos(item.lat * .pi / 180))
                return CLLocationCoordinate2D(latitude: item.lat + dlat, longitude: item.lon + dlon)
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
            MainActor.assumeIsolated { parent.onCamera(bounds, zoom) }
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
        imageView.frame = CGRect(origin: .zero, size: size)
        frame.size = size
        let anchor = a.item.kind == .me ? MarkerBadge.dot / 2 : MarkerBadge.avatar / 2
        centerOffset = CGVector(dx: 0, dy: size.height / 2 - anchor)
        layer.zPosition = a.item.kind == .sos ? 2 : (a.item.kind == .me ? 1 : 0)
    }
}

/// An avatar (or our blue dot) with its label, as in the mockup.
struct MarkerBadge: View {
    static let avatar: CGFloat = 32
    static let dot: CGFloat = 22
    let item: MapItem

    var body: some View {
        VStack(spacing: 4) {
            switch item.kind {
            case .me:
                Circle()
                    .fill(Color(item.color))
                    .frame(width: Self.dot, height: Self.dot)
                    .overlay(Circle().stroke(.white, lineWidth: 3))
            case .peer, .sos:
                Text(item.initials)
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(.white)
                    .frame(width: Self.avatar, height: Self.avatar)
                    .background(Color(item.color), in: Circle())
                    .overlay(Circle().stroke(.white, lineWidth: 2))
            }
            if !item.label.isEmpty {
                Text(item.label)
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(item.kind == .sos ? .white : Color(item.color))
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(item.kind == .sos ? Theme.danger : .white, in: RoundedRectangle(cornerRadius: 6))
                    .fixedSize()
            }
        }
        .padding(2)
        .environment(\.colorScheme, .light)
    }
}

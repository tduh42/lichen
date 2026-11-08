import CoreLocation

/// The phone's location service, on while something needs our position (a share, SOS, the
/// map on screen). Keeps running in the background while sharing (blue indicator).
@MainActor
final class Locator: NSObject, CLLocationManagerDelegate {
    private let manager = CLLocationManager()
    private(set) var running = false
    var onFix: ((CLLocation) -> Void)?
    var onAuthorization: ((CLAuthorizationStatus) -> Void)?
    /// Compass headings, degrees from true north (magnetic when true is unknown).
    var onHeading: ((Double) -> Void)?

    override init() {
        super.init()
        manager.delegate = self
        manager.desiredAccuracy = kCLLocationAccuracyBest
        // Every update: the core decides what is worth sending (dead reckoning, §12.2).
        manager.distanceFilter = kCLDistanceFilterNone
        manager.activityType = .otherNavigation
        manager.pausesLocationUpdatesAutomatically = false
    }

    var authorization: CLAuthorizationStatus { manager.authorizationStatus }

    /// Runs the service; `background`: keep it running when the app is not on screen.
    func start(background: Bool) {
        if manager.authorizationStatus == .notDetermined {
            manager.requestWhenInUseAuthorization()
        }
        manager.allowsBackgroundLocationUpdates = background
        manager.showsBackgroundLocationIndicator = background
        if !running {
            manager.startUpdatingLocation()
            running = true
        }
    }

    func stop() {
        guard running else { return }
        manager.stopUpdatingLocation()
        manager.allowsBackgroundLocationUpdates = false
        running = false
    }

    /// The compass (search bearings), where the phone has one.
    func startHeading() {
        guard CLLocationManager.headingAvailable() else { return }
        manager.headingFilter = 2
        manager.startUpdatingHeading()
    }

    func stopHeading() {
        manager.stopUpdatingHeading()
    }

    nonisolated func locationManager(_ manager: CLLocationManager, didUpdateHeading newHeading: CLHeading) {
        guard newHeading.headingAccuracy >= 0 else { return }
        let h = newHeading.trueHeading >= 0 ? newHeading.trueHeading : newHeading.magneticHeading
        MainActor.assumeIsolated { onHeading?(h) }
    }

    nonisolated func locationManager(_ manager: CLLocationManager, didUpdateLocations locations: [CLLocation]) {
        guard let last = locations.last else { return }
        MainActor.assumeIsolated { onFix?(last) }
    }

    nonisolated func locationManagerDidChangeAuthorization(_ manager: CLLocationManager) {
        let status = manager.authorizationStatus
        MainActor.assumeIsolated { onAuthorization?(status) }
    }

    nonisolated func locationManager(_ manager: CLLocationManager, didFailWithError error: Error) {}
}

extension LocationFix {
    init(_ l: CLLocation) {
        self.init(
            lat: l.coordinate.latitude,
            lon: l.coordinate.longitude,
            altM: l.verticalAccuracy > 0 ? l.altitude : nil,
            accuracyM: max(l.horizontalAccuracy, 1),
            headingDeg: l.course >= 0 ? l.course : nil,
            speedMps: l.speed >= 0 ? l.speed : nil,
            unixS: UInt64(max(0, l.timestamp.timeIntervalSince1970))
        )
    }
}

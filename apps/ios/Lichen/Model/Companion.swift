import CoreBluetooth
import Foundation

/// The MeshCore companion over Bluetooth: Nordic UART service, one write or notification per
/// companion frame. Pairing (passkey shown on the node) is handled by iOS when the encrypted
/// characteristics are first used.
final class Companion: NSObject {
    struct Found: Identifiable, Equatable {
        let id: UUID
        let name: String
        let rssi: Int
    }

    enum State: Equatable {
        case off
        case idle
        case scanning
        case connecting(String)
        case connected(String)
    }

    var onState: ((State) -> Void)?
    var onFound: (([Found]) -> Void)?
    var onFrame: ((Data) -> Void)?

    private let uuids = companionBleUuids()
    private var central: CBCentralManager!
    private var peripheral: CBPeripheral?
    private var rx: CBCharacteristic?
    private var found: [UUID: Found] = [:]
    private var pending: [Data] = []
    private static let lastDevice = "lichen.lastDevice"

    private(set) var state: State = .off {
        didSet { onState?(state) }
    }

    override init() {
        super.init()
        central = CBCentralManager(
            delegate: self,
            queue: .main,
            options: [CBCentralManagerOptionRestoreIdentifierKey: "lichen-central"]
        )
    }

    func scan() {
        guard central.state == .poweredOn else { return }
        found = [:]
        state = .scanning
        central.scanForPeripherals(withServices: [CBUUID(string: uuids.service)])
    }

    func stopScan() {
        central.stopScan()
        if state == .scanning { state = .idle }
    }

    func connect(_ id: UUID) {
        stopScan()
        guard let p = central.retrievePeripherals(withIdentifiers: [id]).first else { return }
        peripheral = p
        p.delegate = self
        state = .connecting(p.name ?? "MeshCore")
        central.connect(p)
        UserDefaults.standard.set(id.uuidString, forKey: Self.lastDevice)
    }

    func disconnect() {
        if let p = peripheral { central.cancelPeripheralConnection(p) }
        UserDefaults.standard.removeObject(forKey: Self.lastDevice)
    }

    /// Writes one companion frame (≤ 176 bytes).
    func write(_ frame: Data) {
        guard let p = peripheral, let rx else {
            pending.append(frame)
            return
        }
        p.writeValue(frame, for: rx, type: .withResponse)
    }

    private func reconnectLast() {
        guard let s = UserDefaults.standard.string(forKey: Self.lastDevice), let id = UUID(uuidString: s)
        else { return }
        connect(id)
    }
}

extension Companion: CBCentralManagerDelegate {
    func centralManagerDidUpdateState(_ central: CBCentralManager) {
        if central.state == .poweredOn {
            if case .off = state { state = .idle }
            reconnectLast()
        } else {
            state = .off
        }
    }

    func centralManager(_ central: CBCentralManager, willRestoreState dict: [String: Any]) {
        if let restored = (dict[CBCentralManagerRestoredStatePeripheralsKey] as? [CBPeripheral])?.first {
            peripheral = restored
            restored.delegate = self
        }
    }

    func centralManager(
        _ central: CBCentralManager, didDiscover peripheral: CBPeripheral,
        advertisementData: [String: Any], rssi RSSI: NSNumber
    ) {
        let name = (advertisementData[CBAdvertisementDataLocalNameKey] as? String) ?? peripheral.name ?? "MeshCore"
        found[peripheral.identifier] = Found(id: peripheral.identifier, name: name, rssi: RSSI.intValue)
        onFound?(found.values.sorted { $0.rssi > $1.rssi })
    }

    func centralManager(_ central: CBCentralManager, didConnect peripheral: CBPeripheral) {
        peripheral.discoverServices([CBUUID(string: uuids.service)])
    }

    func centralManager(_ central: CBCentralManager, didFailToConnect peripheral: CBPeripheral, error: Error?) {
        state = .idle
    }

    func centralManager(_ central: CBCentralManager, didDisconnectPeripheral peripheral: CBPeripheral, error: Error?) {
        rx = nil
        state = .idle
        // Stay connected in the background (ADR-0006): reconnect when the node comes back.
        if UserDefaults.standard.string(forKey: Self.lastDevice) != nil {
            central.connect(peripheral)
            state = .connecting(peripheral.name ?? "MeshCore")
        }
    }
}

extension Companion: CBPeripheralDelegate {
    func peripheral(_ peripheral: CBPeripheral, didDiscoverServices error: Error?) {
        guard let service = peripheral.services?.first(where: { $0.uuid == CBUUID(string: uuids.service) }) else { return }
        peripheral.discoverCharacteristics([CBUUID(string: uuids.rx), CBUUID(string: uuids.tx)], for: service)
    }

    func peripheral(_ peripheral: CBPeripheral, didDiscoverCharacteristicsFor service: CBService, error: Error?) {
        for c in service.characteristics ?? [] {
            if c.uuid == CBUUID(string: uuids.rx) { rx = c }
            if c.uuid == CBUUID(string: uuids.tx) { peripheral.setNotifyValue(true, for: c) }
        }
    }

    func peripheral(_ peripheral: CBPeripheral, didUpdateNotificationStateFor characteristic: CBCharacteristic, error: Error?) {
        guard error == nil, characteristic.isNotifying else { return }
        state = .connected(peripheral.name ?? "MeshCore")
        let queued = pending
        pending = []
        queued.forEach(write)
    }

    func peripheral(_ peripheral: CBPeripheral, didUpdateValueFor characteristic: CBCharacteristic, error: Error?) {
        if let data = characteristic.value, !data.isEmpty { onFrame?(data) }
    }
}

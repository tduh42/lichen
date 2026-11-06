import SwiftUI

/// Colours of the mockups (docs/maquettes/).
enum Theme {
    static let accent = Color(hex: 0xC2410C)
    static let accentSoft = Color(hex: 0xFFEDD5)
    static let accentText = Color(hex: 0x9A3412)
    static let ok = Color(hex: 0x15803D)
    static let danger = Color(hex: 0xB91C1C)
    static let secondaryText = Color(hex: 0x5B5B60)
    static let incomingBubble = Color(hex: 0xE9E9EB)

    struct Chip {
        let label: LocalizedStringKey
        let background: Color
        let foreground: Color
    }

    static let lora = Chip(label: "LoRa", background: accentSoft, foreground: accentText)
    static let iroh = Chip(label: "iroh", background: Color(hex: 0xDBEAFE), foreground: Color(hex: 0x1E40AF))
    static let tor = Chip(label: "Tor", background: Color(hex: 0xEDE9FE), foreground: Color(hex: 0x5B21B6))

    /// Avatar colours, picked from the key so a contact keeps its colour.
    static func avatar(for key: Data) -> (Color, Color) {
        let palette: [(UInt32, UInt32)] = [
            (0xFFEDD5, 0x9A3412), (0xFEE2E2, 0x991B1B), (0xDCFCE7, 0x166534),
            (0xEDE9FE, 0x5B21B6), (0xDBEAFE, 0x1E40AF), (0xE5E7EB, 0x374151),
        ]
        let i = Int(key.first ?? 0) % palette.count
        return (Color(hex: palette[i].0), Color(hex: palette[i].1))
    }
}

extension Color {
    init(hex: UInt32) {
        self.init(
            red: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255
        )
    }
}

/// A small coloured badge ("LoRa · 2 hops").
struct Badge: View {
    let text: String
    let background: Color
    let foreground: Color

    var body: some View {
        Text(text)
            .font(.caption2.weight(.semibold))
            .padding(.horizontal, 8)
            .padding(.vertical, 2)
            .background(background, in: RoundedRectangle(cornerRadius: 8))
            .foregroundStyle(foreground)
    }
}

/// Initials in a coloured circle.
struct Avatar: View {
    let name: String
    let key: Data
    var size: CGFloat = 44

    var body: some View {
        let (bg, fg) = Theme.avatar(for: key)
        Text(initials)
            .font(.system(size: size * 0.34, weight: .semibold))
            .frame(width: size, height: size)
            .background(bg, in: Circle())
            .foregroundStyle(fg)
            .accessibilityHidden(true)
    }

    private var initials: String {
        let words = name.split(separator: " ").prefix(2)
        let letters = words.compactMap(\.first).map(String.init).joined()
        return letters.isEmpty ? "?" : letters.uppercased()
    }
}

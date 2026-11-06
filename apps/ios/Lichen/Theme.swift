import SwiftUI
import UIKit

/// Design tokens (docs/design/tokens.json): dark by default, light as an option. Orange means
/// LoRa and the brand, nothing else; primary actions use `actBg` / `actFg`.
enum Theme {
    static let bg = Color(dark: 0x0E0E0D, light: 0xF7F7F5)
    static let s1 = Color(dark: 0x1A1A18, light: 0xFFFFFF)
    static let s2 = Color(dark: 0x262623, light: 0xEFEFEB)
    static let line = Color(dark: 0x34342F, light: 0xE2E2DC)
    static let tx = Color(dark: 0xF4F4F0, light: 0x141413)
    static let tx2 = Color(dark: 0xABABA3, light: 0x55554F)
    static let actBg = Color(dark: 0xF4F4F0, light: 0x141413)
    static let actFg = Color(dark: 0x141413, light: 0xFFFFFF)

    static let lora = Color(dark: 0xFB923C, light: 0xC2410C)
    static let loraBg = Color(dark: 0x3B2414, light: 0xFFEDD5)
    static let loraFg = Color(dark: 0xFDBA74, light: 0x9A3412)
    static let iroh = Color(dark: 0x60A5FA, light: 0x1D4ED8)
    static let irohBg = Color(dark: 0x16264A, light: 0xDBEAFE)
    static let irohFg = Color(dark: 0x93C5FD, light: 0x1E40AF)
    static let tor = Color(dark: 0xA78BFA, light: 0x6D28D9)
    static let torBg = Color(dark: 0x2A1B4D, light: 0xEDE9FE)
    static let torFg = Color(dark: 0xC4B5FD, light: 0x5B21B6)
    static let sos = Color(dark: 0xEF4444, light: 0xB91C1C)
    static let sosBg = Color(dark: 0x3F1414, light: 0xFEE2E2)
    static let sosFg = Color(dark: 0xFCA5A5, light: 0x991B1B)
    static let sosBtn = Color(dark: 0xDC2626, light: 0xB91C1C)
    static let ok = Color(dark: 0x4ADE80, light: 0x15803D)
    static let okBg = Color(dark: 0x12301C, light: 0xDCFCE7)
    static let okFg = Color(dark: 0x86EFAC, light: 0x166534)
    static let warn = Color(dark: 0xFACC15, light: 0xA16207)
    static let warnBg = Color(dark: 0x332B0A, light: 0xFEF9C3)
    static let warnFg = Color(dark: 0xFDE047, light: 0x854D0E)

    /// Dark map marker blue (our position).
    static let me = Color(dark: 0x60A5FA, light: 0x1D4ED8)

    // Names kept from before the design handoff.
    static let danger = sos
    static let secondaryText = tx2

    /// Avatar colours, picked from the key so a contact keeps its colour.
    static func avatar(for key: Data) -> (Color, Color) {
        let palette: [(Color, Color)] = [
            (loraBg, loraFg), (irohBg, irohFg), (torBg, torFg), (okBg, okFg), (warnBg, warnFg), (s2, tx),
        ]
        return palette[Int(key.first ?? 0) % palette.count]
    }

    /// A solid colour for a key (map markers): the `_fg` of its avatar palette.
    static func marker(for key: Data) -> Color {
        let solid: [Color] = [lora, iroh, tor, ok, warn, tx2]
        return solid[Int(key.first ?? 0) % solid.count]
    }

    /// Big figures (budget, distance, fingerprints): bold, tabular.
    static func figure(_ size: CGFloat) -> Font {
        .system(size: size, weight: .bold).monospacedDigit()
    }

    /// The wordmark.
    static func wordmark(_ size: CGFloat) -> Font {
        .system(size: size, weight: .bold)
    }
}

/// The system's choice, dark or light (Settings); the system's by default.
enum Appearance: String, CaseIterable, Identifiable {
    case system, dark, light

    var id: String { rawValue }

    var scheme: ColorScheme? {
        switch self {
        case .dark: .dark
        case .light: .light
        case .system: nil
        }
    }

    var label: LocalizedStringKey {
        switch self {
        case .dark: "Dark"
        case .light: "Light"
        case .system: "System"
        }
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

    /// A colour that follows the interface style.
    init(dark: UInt32, light: UInt32) {
        self.init(UIColor { traits in
            let hex = traits.userInterfaceStyle == .light ? light : dark
            return UIColor(
                red: CGFloat((hex >> 16) & 0xFF) / 255,
                green: CGFloat((hex >> 8) & 0xFF) / 255,
                blue: CGFloat(hex & 0xFF) / 255,
                alpha: 1
            )
        })
    }
}

// MARK: - surfaces and buttons

extension View {
    /// A card: `s1`, radius 14.
    func card(padding: CGFloat = 14) -> some View {
        self.padding(padding)
            .background(Theme.s1, in: RoundedRectangle(cornerRadius: 14))
    }

    /// Screen background.
    func screen() -> some View {
        self.background(Theme.bg.ignoresSafeArea())
    }
}

/// Primary button: `actBg` / `actFg`, 50 high, radius 14.
struct PrimaryButtonStyle: ButtonStyle {
    @Environment(\.isEnabled) private var enabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 17, weight: .semibold))
            .frame(maxWidth: .infinity, minHeight: 50)
            .background(Theme.actBg, in: RoundedRectangle(cornerRadius: 14))
            .foregroundStyle(Theme.actFg)
            .opacity(enabled ? (configuration.isPressed ? 0.8 : 1) : 0.4)
    }
}

/// Secondary button: `s2`, or a semantic tint (`sos` for destructive).
struct SecondaryButtonStyle: ButtonStyle {
    var background: Color = Theme.s2
    var foreground: Color = Theme.tx
    var height: CGFloat = 50
    @Environment(\.isEnabled) private var enabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 16, weight: .semibold))
            .multilineTextAlignment(.center)
            .frame(maxWidth: .infinity, minHeight: height)
            .padding(.horizontal, 8)
            .background(background, in: RoundedRectangle(cornerRadius: 14))
            .foregroundStyle(foreground)
            .opacity(enabled ? (configuration.isPressed ? 0.8 : 1) : 0.4)
    }
}

extension ButtonStyle where Self == PrimaryButtonStyle {
    static var primary: PrimaryButtonStyle { PrimaryButtonStyle() }
}

extension ButtonStyle where Self == SecondaryButtonStyle {
    static var secondary: SecondaryButtonStyle { SecondaryButtonStyle() }
    static var destructive: SecondaryButtonStyle {
        SecondaryButtonStyle(background: Theme.sosBg, foreground: Theme.sosFg)
    }
}

/// A round 44 pt icon button (`s2`, or `actBg` for the main one).
struct RoundIconButton: View {
    let systemName: String
    let label: LocalizedStringKey
    var prominent = false
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: systemName)
                .font(.system(size: 18, weight: .semibold))
                .frame(width: 44, height: 44)
                .background(prominent ? Theme.actBg : Theme.s2, in: Circle())
                .foregroundStyle(prominent ? Theme.actFg : Theme.tx)
        }
        .accessibilityLabel(label)
    }
}

// MARK: - small views

/// A coloured badge ("LoRa · 2 hops").
struct Badge: View {
    let text: String
    let background: Color
    let foreground: Color

    var body: some View {
        Text(text)
            .font(.system(size: 11, weight: .semibold))
            .lineLimit(1)
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
        Text(initials(name))
            .font(.system(size: size * 0.32, weight: .semibold))
            .frame(width: size, height: size)
            .background(bg, in: Circle())
            .foregroundStyle(fg)
            .accessibilityHidden(true)
    }
}

/// "CA" for "Camille", "RV" for "Rando Vercors".
func initials(_ name: String) -> String {
    let words = name.split(separator: " ")
    let letters = words.count == 1
        ? String(words[0].prefix(2))
        : words.prefix(2).compactMap(\.first).map(String.init).joined()
    return letters.isEmpty ? "?" : letters.uppercased()
}

/// A section label above a card ("Radio", "Members").
struct SectionLabel: View {
    let text: LocalizedStringKey

    init(_ text: LocalizedStringKey) {
        self.text = text
    }

    var body: some View {
        Text(text)
            .font(.system(size: 13))
            .foregroundStyle(Theme.tx2)
            .padding(.horizontal, 4)
            .frame(maxWidth: .infinity, alignment: .leading)
    }
}

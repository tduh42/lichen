// Draws the lichen mark (docs/brand/lichen-mark.svg) as the 1024 px iOS app icons (default,
// dark, tinted). Usage: swift scripts/render-app-icon.swift <AppIcon.appiconset>
import AppKit

func draw(_ path: String, background: NSColor?, mark: NSColor) {
    let size = 1024.0
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(size), pixelsHigh: Int(size),
                               bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                               colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    let ctx = NSGraphicsContext(bitmapImageRep: rep)!
    NSGraphicsContext.current = ctx
    let cg = ctx.cgContext
    if let background {
        cg.setFillColor(background.cgColor)
        cg.fill(CGRect(x: 0, y: 0, width: size, height: size))
    }
    // The 64-unit grid takes 70 % of the tile (140 of 200 in the reference), centred; y down.
    let scale = size * 0.7 / 64
    let offset = (size - 64 * scale) / 2
    cg.translateBy(x: offset, y: size - offset)
    cg.scaleBy(x: scale, y: -scale)
    cg.setStrokeColor(mark.cgColor)
    cg.setLineWidth(5.5)
    cg.setLineJoin(.round)
    cg.setLineCap(.round)
    cg.move(to: CGPoint(x: 10, y: 50))
    cg.addLine(to: CGPoint(x: 24, y: 14))
    cg.addLine(to: CGPoint(x: 24, y: 50))
    cg.addLine(to: CGPoint(x: 38, y: 14))
    cg.addLine(to: CGPoint(x: 38, y: 50))
    cg.addLine(to: CGPoint(x: 48, y: 28.3))
    cg.strokePath()
    cg.setFillColor(mark.cgColor)
    cg.fillEllipse(in: CGRect(x: 53.5 - 4, y: 16.4 - 4, width: 8, height: 8))
    NSGraphicsContext.restoreGraphicsState()
    try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: path))
}

func rgb(_ hex: UInt32) -> NSColor {
    NSColor(srgbRed: CGFloat((hex >> 16) & 0xFF) / 255, green: CGFloat((hex >> 8) & 0xFF) / 255,
            blue: CGFloat(hex & 0xFF) / 255, alpha: 1)
}

let dir = CommandLine.arguments[1]
let ink = rgb(0x141413), signal = rgb(0xFB923C)
draw("\(dir)/icon-1024.png", background: ink, mark: signal)
// iOS 18 dark icon: the system supplies the dark background.
draw("\(dir)/icon-1024-dark.png", background: nil, mark: signal)
// Tinted icon: a grayscale mark the system colours.
draw("\(dir)/icon-1024-tinted.png", background: nil, mark: .white)

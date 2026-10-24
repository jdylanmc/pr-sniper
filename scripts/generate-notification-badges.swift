import AppKit

// Notification state badges: a filled disc with a white glyph and a white rim,
// legible on light and dark notification surfaces. Shape carries the state too,
// so color is never the only cue.
enum Glyph { case check, bang, crosshair, cross }

func png(size: Int, color: NSColor, glyph: Glyph) throws -> Data {
    let image = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size,
        bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
        colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
    )!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: image)
    (AffineTransform(scale: CGFloat(size) / 32) as NSAffineTransform).concat()
    NSColor.white.setFill()
    NSBezierPath(ovalIn: NSRect(x: 1, y: 1, width: 30, height: 30)).fill()
    color.setFill()
    NSBezierPath(ovalIn: NSRect(x: 2.5, y: 2.5, width: 27, height: 27)).fill()
    NSColor.white.setStroke()
    NSColor.white.setFill()
    let path = NSBezierPath()
    path.lineWidth = 3.5
    path.lineCapStyle = .round
    path.lineJoinStyle = .round
    switch glyph {
    case .check:
        path.move(to: NSPoint(x: 9.5, y: 16.5))
        path.line(to: NSPoint(x: 14, y: 11.5))
        path.line(to: NSPoint(x: 22.5, y: 21))
        path.stroke()
    case .bang:
        path.move(to: NSPoint(x: 16, y: 23))
        path.line(to: NSPoint(x: 16, y: 15))
        path.stroke()
        NSBezierPath(ovalIn: NSRect(x: 14, y: 8, width: 4, height: 4)).fill()
    case .crosshair:
        path.lineWidth = 2.5
        path.appendOval(in: NSRect(x: 10.5, y: 10.5, width: 11, height: 11))
        for (start, end) in [
            (NSPoint(x: 16, y: 7), NSPoint(x: 16, y: 11)),
            (NSPoint(x: 16, y: 21), NSPoint(x: 16, y: 25)),
            (NSPoint(x: 7, y: 16), NSPoint(x: 11, y: 16)),
            (NSPoint(x: 21, y: 16), NSPoint(x: 25, y: 16))
        ] {
            path.move(to: start)
            path.line(to: end)
        }
        path.stroke()
    case .cross:
        path.move(to: NSPoint(x: 11, y: 11))
        path.line(to: NSPoint(x: 21, y: 21))
        path.move(to: NSPoint(x: 21, y: 11))
        path.line(to: NSPoint(x: 11, y: 21))
        path.stroke()
    }
    NSGraphicsContext.restoreGraphicsState()
    return image.representation(using: .png, properties: [:])!
}

func rgb(_ hex: Int) -> NSColor {
    NSColor(
        srgbRed: CGFloat((hex >> 16) & 0xff) / 255,
        green: CGFloat((hex >> 8) & 0xff) / 255,
        blue: CGFloat(hex & 0xff) / 255,
        alpha: 1)
}

// 256px covers the macOS attachment thumbnail at 2x and the Windows 48px
// app-logo slot up to 400% scale. Names match `notifications::Badge::file_name`.
let destination = URL(fileURLWithPath: "src-tauri/icons/notifications", isDirectory: true)
try FileManager.default.createDirectory(at: destination, withIntermediateDirectories: true)
for (name, color, glyph) in [
    ("success", rgb(0x1A7F37), Glyph.check),
    ("attention", rgb(0xB76E00), Glyph.bang),
    ("ready", rgb(0x0969DA), Glyph.crosshair),
    ("failure", rgb(0xCF222E), Glyph.cross)
] {
    try png(size: 256, color: color, glyph: glyph)
        .write(to: destination.appendingPathComponent("\(name).png"))
}

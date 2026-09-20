import AppKit

// The canonical crosshair is drawn as geometry, never a private-use font.
func png(size: Int, appIcon: Bool) throws -> Data {
    let image = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size,
        bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
        colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
    )!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: image)
    let scale = CGFloat(size) / 32
    let transform = AffineTransform(scale: scale)
    (transform as NSAffineTransform).concat()
    if appIcon {
        NSColor(calibratedRed: 0.06, green: 0.18, blue: 0.22, alpha: 1).setFill()
        NSBezierPath(roundedRect: NSRect(x: 0, y: 0, width: 32, height: 32), xRadius: 7, yRadius: 7).fill()
        let inset = NSAffineTransform()
        inset.translateX(by: 4, yBy: 4)
        inset.scale(by: 0.75)
        inset.concat()
    }
    (appIcon ? NSColor.white : NSColor.black).setStroke()
    let circle = NSBezierPath(ovalIn: NSRect(x: 6, y: 6, width: 20, height: 20))
    circle.lineWidth = 3
    circle.stroke()
    let ticks = NSBezierPath()
    ticks.lineWidth = 3
    for (start, end) in [
        (NSPoint(x: 16, y: 1), NSPoint(x: 16, y: 10)),
        (NSPoint(x: 16, y: 22), NSPoint(x: 16, y: 31)),
        (NSPoint(x: 1, y: 16), NSPoint(x: 10, y: 16)),
        (NSPoint(x: 22, y: 16), NSPoint(x: 31, y: 16))
    ] {
        ticks.move(to: start)
        ticks.line(to: end)
    }
    ticks.stroke()
    NSGraphicsContext.restoreGraphicsState()
    return image.representation(using: .png, properties: [:])!
}

let destination = URL(fileURLWithPath: "src-tauri/icons", isDirectory: true)
let iconset = destination.appendingPathComponent("icon.iconset", isDirectory: true)
try FileManager.default.createDirectory(at: iconset, withIntermediateDirectories: true)
try png(size: 44, appIcon: false).write(to: destination.appendingPathComponent("tray.png"))
try png(size: 512, appIcon: true).write(to: destination.appendingPathComponent("icon.png"))
for size in [16, 32, 128, 256, 512] {
    try png(size: size, appIcon: true).write(to: iconset.appendingPathComponent("icon_\(size)x\(size).png"))
    try png(size: size * 2, appIcon: true).write(to: iconset.appendingPathComponent("icon_\(size)x\(size)@2x.png"))
}
let process = Process()
process.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
process.arguments = ["-c", "icns", iconset.path, "-o", destination.appendingPathComponent("icon.icns").path]
try process.run()
process.waitUntilExit()
guard process.terminationStatus == 0 else { fatalError("iconutil failed") }
try FileManager.default.removeItem(at: iconset)

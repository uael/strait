// Draws the app icon: a teal tile holding two screens side by side.
// Usage: swift scripts/icon.swift <output.iconset>
import AppKit

let out = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)

func draw(_ size: Int) -> Data {
    let s = CGFloat(size)
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size, bitsPerSample: 8,
                               samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
                               bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    // macOS icon grid: the tile fills about 80% of the canvas.
    let inset = s * 0.1
    let tile = NSRect(x: inset, y: inset, width: s - 2 * inset, height: s - 2 * inset)
    let tilePath = NSBezierPath(roundedRect: tile, xRadius: tile.width * 0.225, yRadius: tile.width * 0.225)
    NSGradient(starting: NSColor(red: 0.08, green: 0.72, blue: 0.70, alpha: 1),
               ending: NSColor(red: 0.10, green: 0.42, blue: 0.78, alpha: 1))!.draw(in: tilePath, angle: -60)
    let gap = tile.width * 0.06
    let screenW = (tile.width * 0.68 - gap) / 2
    let screenH = tile.height * 0.40
    let originX = tile.midX - screenW - gap / 2
    let originY = tile.midY - screenH / 2 + tile.height * 0.04
    for (i, alpha) in [(0, 1.0), (1, 0.82)] {
        let r = NSRect(x: originX + CGFloat(i) * (screenW + gap), y: originY, width: screenW, height: screenH)
        NSColor(white: 1, alpha: alpha).setFill()
        NSBezierPath(roundedRect: r, xRadius: screenW * 0.12, yRadius: screenW * 0.12).fill()
    }
    // The stand under both screens.
    let stand = NSRect(x: tile.midX - tile.width * 0.12, y: originY - tile.height * 0.11, width: tile.width * 0.24, height: tile.height * 0.035)
    NSColor(white: 1, alpha: 0.9).setFill()
    NSBezierPath(roundedRect: stand, xRadius: stand.height / 2, yRadius: stand.height / 2).fill()
    NSGraphicsContext.restoreGraphicsState()
    return rep.representation(using: .png, properties: [:])!
}

for base in [16, 32, 128, 256, 512] {
    try draw(base).write(to: out.appendingPathComponent("icon_\(base)x\(base).png"))
    try draw(base * 2).write(to: out.appendingPathComponent("icon_\(base)x\(base)@2x.png"))
}

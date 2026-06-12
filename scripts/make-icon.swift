// Renders the Muse app icon: a paper squircle with the serif wordmark "M."
// — ink M, rose-clay period — at every size macOS wants in an .icns.
//
// Usage: swift scripts/make-icon.swift <out-dir>
// Writes <out-dir>/Muse.iconset/icon_*.png; run iconutil afterwards.

import AppKit

let args = CommandLine.arguments
guard args.count == 2 else {
    fputs("usage: swift make-icon.swift <out-dir>\n", stderr)
    exit(1)
}
let iconset = URL(fileURLWithPath: args[1]).appendingPathComponent("Muse.iconset")
try? FileManager.default.createDirectory(at: iconset, withIntermediateDirectories: true)

let paper = NSColor(srgbRed: 0.980, green: 0.973, blue: 0.961, alpha: 1.0) // #FAF8F5
let ink = NSColor(srgbRed: 0.149, green: 0.133, blue: 0.110, alpha: 1.0)   // #26221C
let rose = NSColor(srgbRed: 0.722, green: 0.392, blue: 0.314, alpha: 1.0)  // #B86450
let edge = NSColor(srgbRed: 0.149, green: 0.133, blue: 0.110, alpha: 0.10)

func draw(canvas: Int) -> NSImage {
    let s = CGFloat(canvas)
    let image = NSImage(size: NSSize(width: s, height: s))
    image.lockFocus()

    // HIG-style rounded rect: ~82% of canvas, centered.
    let inset = s * 0.09
    let rect = NSRect(x: inset, y: inset, width: s - 2 * inset, height: s - 2 * inset)
    let radius = rect.width * 0.225
    let shape = NSBezierPath(roundedRect: rect, xRadius: radius, yRadius: radius)

    // A soft drop shadow so the paper reads on any wallpaper.
    NSGraphicsContext.current?.saveGraphicsState()
    let shadow = NSShadow()
    shadow.shadowColor = NSColor.black.withAlphaComponent(0.22)
    shadow.shadowBlurRadius = s * 0.028
    shadow.shadowOffset = NSSize(width: 0, height: -s * 0.012)
    shadow.set()
    paper.setFill()
    shape.fill()
    NSGraphicsContext.current?.restoreGraphicsState()

    edge.setStroke()
    shape.lineWidth = max(1, s * 0.004)
    shape.stroke()

    // The wordmark: "M" in ink, "." in rose clay — Georgia, the closest
    // bundled cousin of Literata.
    let fontSize = rect.height * 0.62
    guard let font = NSFont(name: "Georgia", size: fontSize) else { exit(2) }
    let mark = NSMutableAttributedString()
    mark.append(NSAttributedString(string: "M", attributes: [.font: font, .foregroundColor: ink]))
    mark.append(NSAttributedString(string: ".", attributes: [.font: font, .foregroundColor: rose]))
    let bounds = mark.boundingRect(with: NSSize(width: s, height: s))
    let at = NSPoint(
        x: rect.midX - bounds.width / 2 - bounds.origin.x,
        y: rect.midY - bounds.height / 2 - bounds.origin.y + rect.height * 0.02
    )
    mark.draw(at: at)

    image.unlockFocus()
    return image
}

func write(_ image: NSImage, px: Int, name: String) {
    guard let tiff = image.tiffRepresentation,
          let rep = NSBitmapImageRep(data: tiff) else { exit(3) }
    rep.size = NSSize(width: px, height: px)
    guard let resized = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: px, pixelsHigh: px, bitsPerSample: 8,
        samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
        colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
    ) else { exit(3) }
    resized.size = NSSize(width: px, height: px)
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: resized)
    NSGraphicsContext.current?.imageInterpolation = .high
    image.draw(in: NSRect(x: 0, y: 0, width: px, height: px))
    NSGraphicsContext.restoreGraphicsState()
    guard let png = resized.representation(using: .png, properties: [:]) else { exit(3) }
    try? png.write(to: iconset.appendingPathComponent(name))
}

let master = draw(canvas: 1024)
for (points, scales) in [(16, [1, 2]), (32, [1, 2]), (128, [1, 2]), (256, [1, 2]), (512, [1, 2])] {
    for scale in scales {
        let px = points * scale
        let suffix = scale == 1 ? "" : "@2x"
        write(master, px: px, name: "icon_\(points)x\(points)\(suffix).png")
    }
}
print("iconset written to \(iconset.path)")

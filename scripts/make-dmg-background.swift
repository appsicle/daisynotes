// Renders the DMG window background: warm paper, a rose-clay arrow from
// Muse.app to the Applications symlink, and one quiet caption.
//
// Usage: swift make-dmg-background.swift <out.png>   (600x360 @2x = 1200x720)

import AppKit

guard CommandLine.arguments.count == 2 else {
    fputs("usage: swift make-dmg-background.swift <out.png>\n", stderr)
    exit(1)
}

let size = NSSize(width: 600, height: 360)
let scale: CGFloat = 2.0
let paper = NSColor(srgbRed: 0.980, green: 0.973, blue: 0.961, alpha: 1.0)
let ink3 = NSColor(srgbRed: 0.659, green: 0.635, blue: 0.588, alpha: 1.0)
let rose = NSColor(srgbRed: 0.722, green: 0.392, blue: 0.314, alpha: 1.0)

let rep = NSBitmapImageRep(
    bitmapDataPlanes: nil,
    pixelsWide: Int(size.width * scale), pixelsHigh: Int(size.height * scale),
    bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
    colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
)!
rep.size = size
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)

paper.setFill()
NSRect(origin: .zero, size: size).fill()

// The arrow: a gentle line from the app icon position to Applications,
// drawn between where the icons will sit (150,190) → (450,190).
let arrow = NSBezierPath()
arrow.move(to: NSPoint(x: 235, y: 175))
arrow.line(to: NSPoint(x: 355, y: 175))
arrow.lineWidth = 3
arrow.lineCapStyle = .round
rose.withAlphaComponent(0.65).setStroke()
arrow.stroke()
let head = NSBezierPath()
head.move(to: NSPoint(x: 341, y: 186))
head.line(to: NSPoint(x: 357, y: 175))
head.line(to: NSPoint(x: 341, y: 164))
head.lineWidth = 3
head.lineCapStyle = .round
head.lineJoinStyle = .round
rose.withAlphaComponent(0.65).setStroke()
head.stroke()

// Caption, set in Georgia italic like the marginalia.
let caption = "drag Muse into Applications"
let font = NSFont(name: "Georgia-Italic", size: 15) ?? NSFont.systemFont(ofSize: 15)
let attrs: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: ink3]
let text = NSAttributedString(string: caption, attributes: attrs)
let bounds = text.boundingRect(with: size)
text.draw(at: NSPoint(x: (size.width - bounds.width) / 2, y: 64))

NSGraphicsContext.restoreGraphicsState()
let png = rep.representation(using: .png, properties: [:])!
try! png.write(to: URL(fileURLWithPath: CommandLine.arguments[1]))
print("background written")

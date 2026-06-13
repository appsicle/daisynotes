// Renders the DMG window background to match the marketing site: warm paper
// with a soft sheen, a glossy light-blue arrow from DaisyNotes.app to the
// Applications symlink, and one clean caption in the modernist sans.
//
// Usage: swift make-dmg-background.swift <out.png>   (600x360 @2x = 1200x720)

import AppKit

guard CommandLine.arguments.count == 2 else {
    fputs("usage: swift make-dmg-background.swift <out.png>\n", stderr)
    exit(1)
}

let size = NSSize(width: 600, height: 360)
let scale: CGFloat = 2.0

// Tokens lifted straight from the marketing site (src/style.css).
let paper        = NSColor(srgbRed: 0.980, green: 0.973, blue: 0.957, alpha: 1.0)  // #FAF8F4
let paperTop     = NSColor(srgbRed: 0.996, green: 0.992, blue: 0.984, alpha: 1.0)  // lighter sheen
let ink3         = NSColor(srgbRed: 0.612, green: 0.584, blue: 0.549, alpha: 1.0)  // #9C958C
let accent       = NSColor(srgbRed: 0.118, green: 0.565, blue: 1.000, alpha: 1.0)  // #1E90FF
let accentStrong = NSColor(srgbRed: 0.082, green: 0.467, blue: 0.902, alpha: 1.0)  // #1577E6
let highlight    = NSColor(srgbRed: 0.560, green: 0.800, blue: 1.000, alpha: 1.0)  // glossy top edge

let rep = NSBitmapImageRep(
    bitmapDataPlanes: nil,
    pixelsWide: Int(size.width * scale), pixelsHigh: Int(size.height * scale),
    bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
    colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
)!
rep.size = size
NSGraphicsContext.saveGraphicsState()
let nsCtx = NSGraphicsContext(bitmapImageRep: rep)!
NSGraphicsContext.current = nsCtx
let ctx = nsCtx.cgContext

// ── Paper: a soft top-to-bottom sheen so the window doesn't read flat ──
let paperGrad = NSGradient(colors: [paperTop, paper])!
paperGrad.draw(in: NSRect(origin: .zero, size: size), angle: -90)

// ── A faint blue glow pooled behind the arrow, tying it to the accent ──
ctx.saveGState()
let glow = CGGradient(
    colorsSpace: CGColorSpaceCreateDeviceRGB(),
    colors: [accent.withAlphaComponent(0.10).cgColor,
             accent.withAlphaComponent(0.0).cgColor] as CFArray,
    locations: [0, 1]
)!
ctx.drawRadialGradient(
    glow,
    startCenter: CGPoint(x: 300, y: 178), startRadius: 0,
    endCenter:   CGPoint(x: 300, y: 178), endRadius: 150,
    options: []
)
ctx.restoreGState()

// ── The glossy arrow: shaft + head, lit from above ────────────────────
// Icons sit at (150,185) and (450,185) in Finder's top-left space, which is
// y≈175 from the bottom here. The arrow bridges the gap between them.
let shaft = CGMutablePath()
shaft.move(to: CGPoint(x: 242, y: 175))
shaft.addLine(to: CGPoint(x: 350, y: 175))
let shaftSolid = shaft.copy(
    strokingWithWidth: 6, lineCap: .round, lineJoin: .round, miterLimit: 10
)

let head = CGMutablePath()
head.move(to: CGPoint(x: 344, y: 190))
head.addLine(to: CGPoint(x: 366, y: 175))
head.addLine(to: CGPoint(x: 344, y: 160))
head.closeSubpath()

let arrow = CGMutablePath()
arrow.addPath(shaftSolid)
arrow.addPath(head)

// Soft drop shadow under the arrow.
ctx.saveGState()
ctx.setShadow(
    offset: CGSize(width: 0, height: -3), blur: 8,
    color: accentStrong.withAlphaComponent(0.40).cgColor
)
ctx.addPath(arrow)
ctx.setFillColor(accentStrong.cgColor)
ctx.fillPath()
ctx.restoreGState()

// Glossy vertical gradient inside the arrow: bright top → strong bottom.
ctx.saveGState()
ctx.addPath(arrow)
ctx.clip()
let body = CGGradient(
    colorsSpace: CGColorSpaceCreateDeviceRGB(),
    colors: [highlight.cgColor, accent.cgColor, accentStrong.cgColor] as CFArray,
    locations: [0.0, 0.45, 1.0]
)!
ctx.drawLinearGradient(
    body,
    start: CGPoint(x: 300, y: 191), end: CGPoint(x: 300, y: 159),
    options: []
)
// A crisp specular highlight skimming the top edge of the shaft.
ctx.setStrokeColor(NSColor.white.withAlphaComponent(0.55).cgColor)
ctx.setLineWidth(1.4)
ctx.setLineCap(.round)
ctx.move(to: CGPoint(x: 247, y: 177.4))
ctx.addLine(to: CGPoint(x: 345, y: 177.4))
ctx.strokePath()
ctx.restoreGState()

// ── Caption, set in the modernist system sans like the marketing copy ──
let caption = "Drag Daisy Notes into Applications"
let font = NSFont.systemFont(ofSize: 15, weight: .regular)
let para = NSMutableParagraphStyle()
para.alignment = .center
let attrs: [NSAttributedString.Key: Any] = [
    .font: font,
    .foregroundColor: ink3,
    .kern: 0.2,
    .paragraphStyle: para,
]
let text = NSAttributedString(string: caption, attributes: attrs)
let bounds = text.boundingRect(with: size)
text.draw(at: NSPoint(x: (size.width - bounds.width) / 2, y: 62))

NSGraphicsContext.restoreGraphicsState()
let png = rep.representation(using: .png, properties: [:])!
try! png.write(to: URL(fileURLWithPath: CommandLine.arguments[1]))
print("background written")

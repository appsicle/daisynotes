// Renders the Daisy Notes app icon from assets/icon/daisynotes-icon.png — the
// "single sheet of paper" mark (a tilted cream sheet with hand-drawn graphite
// lines) on a vibrant blue gradient ground — masked into the macOS squircle
// with a soft contact shadow, at every size macOS wants in an .icns.
//
// The master art lives in assets/icon/daisynotes-icon.svg; regenerate the PNG with:
//   rsvg-convert -w 2048 -h 2048 assets/icon/daisynotes-icon.svg -o assets/icon/daisynotes-icon.png
//
// Usage: swift scripts/make-icon.swift <out-dir>
// Writes <out-dir>/DaisyNotes.iconset/icon_*.png; run iconutil afterwards.

import AppKit

let args = CommandLine.arguments
guard args.count == 2 else {
    fputs("usage: swift make-icon.swift <out-dir>\n", stderr)
    exit(1)
}
let iconset = URL(fileURLWithPath: args[1]).appendingPathComponent("DaisyNotes.iconset")
try? FileManager.default.createDirectory(at: iconset, withIntermediateDirectories: true)

// Find the master art: an env override, then relative to the CWD (package.sh
// cd's to the repo root), then relative to this script's own location.
func resolveMaster() -> URL? {
    let fm = FileManager.default
    let cwd = URL(fileURLWithPath: fm.currentDirectoryPath)
    var candidates: [URL] = []
    if let env = ProcessInfo.processInfo.environment["DAISYNOTES_ICON_MASTER"], !env.isEmpty {
        candidates.append(URL(fileURLWithPath: env))
    }
    candidates.append(cwd.appendingPathComponent("assets/icon/daisynotes-icon.png"))
    let script = URL(fileURLWithPath: #filePath, relativeTo: cwd).standardizedFileURL
    candidates.append(script.deletingLastPathComponent()
        .deletingLastPathComponent()
        .appendingPathComponent("assets/icon/daisynotes-icon.png"))
    return candidates.first { fm.fileExists(atPath: $0.path) }
}

guard let masterURL = resolveMaster(), let master = NSImage(contentsOf: masterURL) else {
    fputs("error: cannot find assets/icon/daisynotes-icon.png (run rsvg-convert first)\n", stderr)
    exit(2)
}

// Renders one square icon at `canvas` px. The master art is already a finished
// squircle with transparent corners and its own glossy rim, so we don't re-mask
// it — we just centre it on the macOS icon grid (~88% of the canvas) and cast a
// soft drop shadow that follows the art's own alpha.
func render(canvas: Int) -> NSBitmapImageRep {
    let s = CGFloat(canvas)
    guard let rep = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: canvas, pixelsHigh: canvas, bitsPerSample: 8,
        samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
        colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
    ) else { exit(3) }
    rep.size = NSSize(width: s, height: s)

    let ctx = NSGraphicsContext(bitmapImageRep: rep)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = ctx
    ctx.imageInterpolation = .high

    // A little breathing room so the tile sits on the macOS grid like a native
    // icon (the art already carries ~4% of its own margin).
    let inset = s * 0.06
    let rect = NSRect(x: inset, y: inset, width: s - 2 * inset, height: s - 2 * inset)

    // Soft drop shadow (cast by the art's alpha) so it lifts off any wallpaper.
    ctx.saveGraphicsState()
    let shadow = NSShadow()
    shadow.shadowColor = NSColor.black.withAlphaComponent(0.26)
    shadow.shadowBlurRadius = s * 0.022
    shadow.shadowOffset = NSSize(width: 0, height: -s * 0.012)
    shadow.set()
    master.draw(in: rect, from: .zero, operation: .sourceOver, fraction: 1.0)
    ctx.restoreGraphicsState()

    NSGraphicsContext.restoreGraphicsState()
    return rep
}

func write(_ rep: NSBitmapImageRep, name: String) {
    guard let png = rep.representation(using: .png, properties: [:]) else { exit(4) }
    try? png.write(to: iconset.appendingPathComponent(name))
}

for (points, scales) in [(16, [1, 2]), (32, [1, 2]), (128, [1, 2]), (256, [1, 2]), (512, [1, 2])] {
    for scale in scales {
        let px = points * scale
        let suffix = scale == 1 ? "" : "@2x"
        write(render(canvas: px), name: "icon_\(points)x\(points)\(suffix).png")
    }
}
print("iconset written to \(iconset.path)")

import AppKit
import SwiftUI

/// Renders the live burn-rate sparkline used as the menu-bar icon. Color is
/// tied to the number next to it: the graph's peak takes the current throttle
/// color (`heat`), and lower points grade cooler — so a red ▲2.7× makes a red
/// graph, a coasting ▼ makes a teal one, while the shape still shows the burn.
/// Returns a colored (non-template) NSImage so it shows in color in the bar.
enum BurnGraph {
    static func image(_ samples: [Double], heat: Double, width: CGFloat = 34, height: CGFloat = 16) -> NSImage {
        let img = NSImage(size: NSSize(width: width, height: height))
        img.lockFocus()
        defer {
            img.unlockFocus()
            img.isTemplate = false
        }
        guard let ctx = NSGraphicsContext.current?.cgContext else { return img }

        let pad: CGFloat = 1
        let w = width - pad * 2
        let h = height - pad * 2
        let maxV = max(samples.max() ?? 0, 1)
        let n = samples.count

        // Baseline so an idle graph still reads as a thin line.
        ctx.setStrokeColor(Palette.dim.rgbaColor.withAlphaComponent(0.25).cgColor)
        ctx.setLineWidth(1)
        ctx.move(to: CGPoint(x: pad, y: pad + 0.5))
        ctx.addLine(to: CGPoint(x: pad + w, y: pad + 0.5))
        ctx.strokePath()

        guard n >= 2 else { return img }

        func point(_ i: Int) -> CGPoint {
            CGPoint(x: pad + w * CGFloat(i) / CGFloat(n - 1),
                    y: pad + h * CGFloat(samples[i] / maxV))
        }
        // Point color = throttle heat scaled by the point's relative height, so
        // the peak matches the number's color and quieter moments read cooler.
        func color(_ i: Int) -> NSColor {
            NSColor(Gov.heatColor(max(0, heat) * samples[i] / maxV))
        }

        // Draw each segment with the color of its (right-hand) value: a filled
        // area quad plus the line on top.
        for i in 0..<(n - 1) {
            let p0 = point(i), p1 = point(i + 1)
            let col = color(i + 1)
            ctx.beginPath()
            ctx.move(to: CGPoint(x: p0.x, y: pad))
            ctx.addLine(to: p0)
            ctx.addLine(to: p1)
            ctx.addLine(to: CGPoint(x: p1.x, y: pad))
            ctx.closePath()
            ctx.setFillColor(col.withAlphaComponent(0.32).cgColor)
            ctx.fillPath()

            ctx.beginPath()
            ctx.move(to: p0)
            ctx.addLine(to: p1)
            ctx.setStrokeColor(col.cgColor)
            ctx.setLineWidth(1.4)
            ctx.setLineCap(.round)
            ctx.strokePath()
        }
        return img
    }
}

private extension Color {
    var rgbaColor: NSColor { NSColor(self) }
}

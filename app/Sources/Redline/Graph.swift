import AppKit
import SwiftUI

/// Renders the live burn-rate sparkline used as the menu-bar icon. Returns a
/// colored (non-template) NSImage so it shows in color in the status bar.
enum BurnGraph {
    static func image(_ samples: [Double], color: Color, width: CGFloat = 34, height: CGFloat = 16) -> NSImage {
        let ns = NSColor(color)
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
        ctx.setStrokeColor(ns.withAlphaComponent(0.25).cgColor)
        ctx.setLineWidth(1)
        ctx.move(to: CGPoint(x: pad, y: pad + 0.5))
        ctx.addLine(to: CGPoint(x: pad + w, y: pad + 0.5))
        ctx.strokePath()

        guard n >= 2 else { return img }

        func point(_ i: Int) -> CGPoint {
            let x = pad + w * CGFloat(i) / CGFloat(n - 1)
            let y = pad + h * CGFloat(samples[i] / maxV)
            return CGPoint(x: x, y: y)
        }

        // Filled area under the curve.
        ctx.beginPath()
        ctx.move(to: CGPoint(x: pad, y: pad))
        for i in 0..<n { ctx.addLine(to: point(i)) }
        ctx.addLine(to: CGPoint(x: pad + w, y: pad))
        ctx.closePath()
        ctx.setFillColor(ns.withAlphaComponent(0.35).cgColor)
        ctx.fillPath()

        // Line on top.
        ctx.beginPath()
        ctx.move(to: point(0))
        for i in 1..<n { ctx.addLine(to: point(i)) }
        ctx.setStrokeColor(ns.cgColor)
        ctx.setLineWidth(1.4)
        ctx.setLineJoin(.round)
        ctx.strokePath()

        return img
    }
}

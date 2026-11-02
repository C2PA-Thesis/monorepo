import SwiftUI

// Small drawings for the first-run pages: ink outlines, flat iris colours,
// and one short motion each. With Reduce Motion they appear in their end state.

private let amber = Color(rgb: 0xFFCF7B)
private let teal = Color(rgb: 0x7BE0D3)
private let blue = Color(rgb: 0x4AA8FF)
private let violet = Color(rgb: 0x8B7BFF)

/// A photo that gets its verified mark.
struct VerifiedPhotoArt: View {
    @State private var card = false
    @State private var badge = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        ZStack {
            PhotoCard()
                .frame(width: 190, height: 232)
                .rotationEffect(.degrees(card ? -6 : 3))
                .offset(y: card ? 0 : 36)
                .opacity(card ? 1 : 0)
            VerifiedMark(size: 64)
                .offset(x: 86, y: -104)
                .scaleEffect(badge ? 1 : 0.2)
                .opacity(badge ? 1 : 0)
        }
        .onAppear {
            if reduceMotion {
                card = true
                badge = true
                return
            }
            withAnimation(.spring(duration: 0.8, bounce: 0.25)) { card = true }
            withAnimation(.spring(duration: 0.6, bounce: 0.5).delay(0.55)) { badge = true }
        }
        .accessibilityHidden(true)
    }
}

private struct PhotoCard: View {
    var body: some View {
        VStack(spacing: 0) {
            GeometryReader { geometry in
                let w = geometry.size.width
                let h = geometry.size.height
                ZStack(alignment: .topLeading) {
                    Theme.surface
                    Circle().fill(amber).frame(width: w * 0.24).offset(x: w * 0.62, y: h * 0.14)
                    Path { path in
                        path.move(to: CGPoint(x: -4, y: h))
                        path.addLine(to: CGPoint(x: w * 0.36, y: h * 0.42))
                        path.addLine(to: CGPoint(x: w * 0.72, y: h))
                    }
                    .fill(teal)
                    Path { path in
                        path.move(to: CGPoint(x: w * 0.38, y: h))
                        path.addLine(to: CGPoint(x: w * 0.7, y: h * 0.56))
                        path.addLine(to: CGPoint(x: w + 4, y: h))
                    }
                    .fill(blue)
                }
            }
            .clipShape(RoundedRectangle(cornerRadius: 8))
            .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Theme.ink, lineWidth: 2.5))
            .padding([.horizontal, .top], 12)
            Color.clear.frame(height: 40)
        }
        .background(Theme.paper, in: RoundedRectangle(cornerRadius: 14))
        .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(Theme.ink, lineWidth: 3))
    }
}

/// A pin that drops, gets wrapped in an area, and disappears into it.
struct PrivateAreaArt: View {
    @State private var pin = false
    @State private var drawn = false
    @State private var hidden = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        ZStack {
            StreetTile().frame(width: 250, height: 210)
            Hexagon()
                .fill(Theme.signalBright.opacity(hidden ? 0.22 : 0))
                .frame(width: 128, height: 128)
            Hexagon()
                .trim(from: 0, to: drawn ? 1 : 0)
                .stroke(Theme.signal, style: StrokeStyle(lineWidth: 3.5, lineCap: .round, lineJoin: .round))
                .frame(width: 128, height: 128)
            Image(systemName: "mappin")
                .font(.system(size: 40, weight: .semibold))
                .foregroundStyle(Theme.ink)
                .offset(y: pin ? -16 : -90)
                .opacity(hidden ? 0 : pin ? 1 : 0)
                .blur(radius: hidden ? 6 : 0)
                .scaleEffect(hidden ? 0.6 : 1)
        }
        .onAppear {
            if reduceMotion {
                drawn = true
                hidden = true
                return
            }
            withAnimation(.spring(duration: 0.6, bounce: 0.4).delay(0.2)) { pin = true }
            withAnimation(.easeInOut(duration: 0.9).delay(0.8)) { drawn = true }
            withAnimation(Theme.ease.speed(0.5).delay(1.8)) { hidden = true }
        }
        .accessibilityHidden(true)
    }
}

private struct StreetTile: View {
    var body: some View {
        RoundedRectangle(cornerRadius: 22).fill(Theme.surface)
            .overlay {
                GeometryReader { geometry in
                    let w = geometry.size.width
                    let h = geometry.size.height
                    Path { path in
                        for x in [0.22, 0.5, 0.8] {
                            path.move(to: CGPoint(x: w * x, y: 0))
                            path.addLine(to: CGPoint(x: w * x + 18, y: h))
                        }
                        for y in [0.25, 0.62] {
                            path.move(to: CGPoint(x: 0, y: h * y))
                            path.addLine(to: CGPoint(x: w, y: h * y - 14))
                        }
                    }
                    .stroke(Theme.line2, style: StrokeStyle(lineWidth: 7, lineCap: .round))
                }
            }
            .clipShape(RoundedRectangle(cornerRadius: 22))
            .overlay(RoundedRectangle(cornerRadius: 22).strokeBorder(Theme.ink, lineWidth: 3))
    }
}

/// A pointy-top hexagon, the shape of a shared area.
struct Hexagon: Shape {
    func path(in rect: CGRect) -> Path {
        let center = CGPoint(x: rect.midX, y: rect.midY)
        let radius = min(rect.width, rect.height) / 2
        var path = Path()
        for index in 0..<6 {
            let angle = -Double.pi / 2 + Double(index) * .pi / 3
            let point = CGPoint(x: center.x + radius * cos(angle), y: center.y + radius * sin(angle))
            if index == 0 { path.move(to: point) } else { path.addLine(to: point) }
        }
        path.closeSubpath()
        return path
    }
}

/// A location dot sending out rings, or still and crossed out when location is off.
struct LocationArt: View {
    var off = false
    @State private var pulse = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        ZStack {
            ForEach(0..<2) { ring in
                Circle().strokeBorder(Theme.signal.opacity(0.5), lineWidth: 2)
                    .frame(width: 96, height: 96)
                    .scaleEffect(pulse ? 2.3 : 1)
                    .opacity(off || pulse ? 0 : 1)
                    .animation(
                        reduceMotion ? nil : .easeOut(duration: 2.4).repeatForever(autoreverses: false).delay(Double(ring) * 1.2),
                        value: pulse)
            }
            Image(systemName: off ? "location.slash.fill" : "location.fill").font(.system(size: 36, weight: .semibold))
                .foregroundStyle(off ? Theme.ink : .white)
                .frame(width: 96, height: 96)
                .background(off ? Theme.surface2 : Theme.signal, in: Circle())
                .overlay(Circle().strokeBorder(Theme.ink, lineWidth: 3))
        }
        .frame(width: 240, height: 240)
        .onAppear { if !reduceMotion, !off { pulse = true } }
        .accessibilityHidden(true)
    }
}

/// The mark, opening and closing like a shutter.
struct ShutterArt: View {
    @State private var open = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        ApertureMark(opening: open ? 0.56 : 0.14, rotation: open ? 0 : -.pi / 4)
            .frame(width: 168, height: 168)
            .onAppear {
                if reduceMotion {
                    open = true
                } else {
                    withAnimation(.easeInOut(duration: 1.4).repeatForever(autoreverses: true)) { open = true }
                }
            }
            .accessibilityHidden(true)
    }
}

/// A laptop and an iPhone, with dots running between them while they connect.
struct ConnectArt: View {
    var active = true
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        HStack(spacing: 18) {
            Image(systemName: "iphone").font(.system(size: 60, weight: .regular))
            TimelineView(.periodic(from: .now, by: 0.35)) { context in
                let tick = !active ? 0 : reduceMotion ? 2 : Int(context.date.timeIntervalSinceReferenceDate / 0.35) % 4
                HStack(spacing: 8) {
                    ForEach(0..<3) { dot in
                        Circle().fill(dot < tick ? violet : Theme.line2).frame(width: 9, height: 9)
                    }
                }
                .animation(Theme.ease, value: tick)
            }
            Image(systemName: "laptopcomputer").font(.system(size: 72, weight: .regular))
        }
        .foregroundStyle(Theme.ink)
        .accessibilityHidden(true)
    }
}

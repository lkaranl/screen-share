import SwiftUI

struct HUDOverlayView: View {
    let fps: Int
    let latency: UInt32

    private var statusColor: Color {
        if latency <= 15 {
            return Color(red: 0.2, green: 0.9, blue: 0.4) // Verde
        } else if latency <= 40 {
            return Color(red: 0.95, green: 0.75, blue: 0.2) // Amarelo
        } else {
            return Color(red: 0.95, green: 0.25, blue: 0.25) // Vermelho
        }
    }

    var body: some View {
        HStack(spacing: 6) {
            Circle()
                .fill(statusColor)
                .frame(width: 7, height: 7)
                .shadow(color: statusColor.opacity(0.6), radius: 3)

            Text("\(fps) FPS")
                .font(.system(size: 11, weight: .bold, design: .monospaced))
                .foregroundColor(.white)

            Text("|")
                .font(.system(size: 10, weight: .light))
                .foregroundColor(.gray)

            Text("\(latency) ms")
                .font(.system(size: 11, weight: .bold, design: .monospaced))
                .foregroundColor(.white)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 5)
        .background(
            RoundedRectangle(cornerRadius: 6, style: .continuous)
                .fill(Color(red: 0.05, green: 0.07, blue: 0.11).opacity(0.85))
                .overlay(
                    RoundedRectangle(cornerRadius: 6, style: .continuous)
                        .stroke(Color.white.opacity(0.12), lineWidth: 1)
                )
        )
        .shadow(color: Color.black.opacity(0.3), radius: 5, x: 0, y: 2)
    }
}

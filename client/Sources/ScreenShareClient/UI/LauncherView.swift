import SwiftUI

struct LauncherView: View {
    let onConnect: (String, VideoCodecType) -> Void

    @AppStorage("last_ip") private var lastIP: String = ""
    @AppStorage("last_codec") private var lastCodecRaw: String = "h264"
    @State private var ipText: String = ""
    @State private var selectedCodec: VideoCodecType = .h264
    @State private var history: [String] = []
    @FocusState private var isFieldFocused: Bool

    private let historyKey = "connection_history"

    var body: some View {
        VStack(spacing: 20) {
            // Header
            VStack(spacing: 6) {
                HStack(spacing: 8) {
                    Image(systemName: "bolt.fill")
                        .font(.system(size: 26))
                        .foregroundColor(Color(red: 0.0, green: 0.8, blue: 1.0))
                        .shadow(color: Color(red: 0.0, green: 0.8, blue: 1.0).opacity(0.5), radius: 8)

                    Text("RS-VIEW")
                        .font(.system(size: 26, weight: .black, design: .rounded))
                        .foregroundColor(.white)
                }

                Text("Streaming Nativo de Ultra-Baixa Latência")
                    .font(.system(size: 11, weight: .medium))
                    .foregroundColor(Color.white.opacity(0.5))
            }
            .padding(.top, 10)

            // Card Principal: Entrada de IP e Codec
            VStack(alignment: .leading, spacing: 14) {
                VStack(alignment: .leading, spacing: 6) {
                    Text("ENDEREÇO IP DO HOST")
                        .font(.system(size: 10, weight: .bold))
                        .foregroundColor(Color(red: 0.0, green: 0.75, blue: 1.0))

                    HStack {
                        Image(systemName: "network")
                            .foregroundColor(Color.white.opacity(0.4))
                            .font(.system(size: 14))

                        TextField("ex: 192.168.68.51", text: $ipText)
                            .font(.system(size: 14, design: .monospaced))
                            .textFieldStyle(PlainTextFieldStyle())
                            .focused($isFieldFocused)
                            .onSubmit {
                                triggerConnect()
                            }

                        if !ipText.isEmpty {
                            Button(action: { ipText = "" }) {
                                Image(systemName: "xmark.circle.fill")
                                    .foregroundColor(Color.white.opacity(0.3))
                            }
                            .buttonStyle(PlainButtonStyle())
                        }

                        Button(action: pasteFromClipboard) {
                            Text("Colar")
                                .font(.system(size: 10, weight: .semibold))
                                .padding(.horizontal, 8)
                                .padding(.vertical, 4)
                                .background(Color.white.opacity(0.1))
                                .cornerRadius(4)
                        }
                        .buttonStyle(PlainButtonStyle())
                    }
                    .padding(10)
                    .background(Color(red: 0.08, green: 0.10, blue: 0.14))
                    .cornerRadius(8)
                    .overlay(
                        RoundedRectangle(cornerRadius: 8)
                            .stroke(Color.white.opacity(0.12), lineWidth: 1)
                    )
                }

                VStack(alignment: .leading, spacing: 6) {
                    Text("CODEC DE VÍDEO")
                        .font(.system(size: 10, weight: .bold))
                        .foregroundColor(Color.white.opacity(0.5))

                    HStack(spacing: 10) {
                        codecButton(title: "🚀 H.264 (Baixa Latência)", type: .h264)
                        codecButton(title: "💎 HEVC (H.265)", type: .hevc)
                    }
                }
            }
            .padding(16)
            .background(Color(red: 0.12, green: 0.14, blue: 0.19))
            .cornerRadius(12)
            .overlay(
                RoundedRectangle(cornerRadius: 12)
                    .stroke(Color.white.opacity(0.08), lineWidth: 1)
            )

            // Botão Principal Conectar
            Button(action: triggerConnect) {
                HStack {
                    Text("CONECTAR AO SERVIDOR")
                        .font(.system(size: 13, weight: .bold))
                    Image(systemName: "arrow.right")
                        .font(.system(size: 12, weight: .bold))
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 12)
                .background(
                    LinearGradient(
                        colors: [Color(red: 0.0, green: 0.48, blue: 1.0), Color(red: 0.0, green: 0.38, blue: 0.9)],
                        startPoint: .top,
                        endPoint: .bottom
                    )
                )
                .foregroundColor(.white)
                .cornerRadius(8)
                .shadow(color: Color.blue.opacity(0.4), radius: 6, x: 0, y: 3)
            }
            .buttonStyle(PlainButtonStyle())

            // Servidores Recentes
            if !history.isEmpty {
                VStack(alignment: .leading, spacing: 8) {
                    Text("SERVIDORES RECENTES")
                        .font(.system(size: 10, weight: .bold))
                        .foregroundColor(Color.white.opacity(0.4))

                    ScrollView {
                        VStack(spacing: 6) {
                            ForEach(history, id: \.self) { server in
                                HStack {
                                    Image(systemName: "desktopcomputer")
                                        .font(.system(size: 12))
                                        .foregroundColor(Color.white.opacity(0.6))

                                    Text(server)
                                        .font(.system(size: 12, design: .monospaced))
                                        .foregroundColor(Color.white.opacity(0.9))

                                    Spacer()

                                    Button(action: { removeHistory(server) }) {
                                        Image(systemName: "trash")
                                            .font(.system(size: 10))
                                            .foregroundColor(Color.red.opacity(0.6))
                                    }
                                    .buttonStyle(PlainButtonStyle())
                                }
                                .padding(.horizontal, 10)
                                .padding(.vertical, 7)
                                .background(Color.white.opacity(0.04))
                                .cornerRadius(6)
                                .contentShape(Rectangle())
                                .onTapGesture {
                                    ipText = server
                                    triggerConnect()
                                }
                            }
                        }
                    }
                    .frame(maxHeight: 110)
                }
            }

            Spacer()

            Text("Portas padrão: 5000 (Vídeo) • 5001 (Input)")
                .font(.system(size: 10))
                .foregroundColor(Color.white.opacity(0.3))
        }
        .padding(24)
        .frame(width: 420, height: 490)
        .background(Color(red: 0.07, green: 0.08, blue: 0.12))
        .onAppear {
            loadConfig()
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) {
                isFieldFocused = true
            }
        }
    }

    private func codecButton(title: String, type: VideoCodecType) -> some View {
        let isSelected = selectedCodec == type
        return Button(action: { selectedCodec = type }) {
            Text(title)
                .font(.system(size: 11, weight: isSelected ? .bold : .medium))
                .foregroundColor(isSelected ? .white : Color.white.opacity(0.6))
                .frame(maxWidth: .infinity)
                .padding(.vertical, 8)
                .background(isSelected ? Color(red: 0.0, green: 0.45, blue: 0.9) : Color.white.opacity(0.06))
                .cornerRadius(6)
                .overlay(
                    RoundedRectangle(cornerRadius: 6)
                        .stroke(isSelected ? Color.white.opacity(0.3) : Color.clear, lineWidth: 1)
                )
        }
        .buttonStyle(PlainButtonStyle())
    }

    private func pasteFromClipboard() {
        if let str = NSPasteboard.general.string(forType: .string) {
            ipText = str.trimmingCharacters(in: .whitespacesAndNewlines)
        }
    }

    private func loadConfig() {
        ipText = lastIP
        if lastCodecRaw == "hevc" {
            selectedCodec = .hevc
        } else {
            selectedCodec = .h264
        }
        if let saved = UserDefaults.standard.stringArray(forKey: historyKey) {
            history = saved
        }
    }

    private func triggerConnect() {
        let trimmed = ipText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }

        lastIP = trimmed
        lastCodecRaw = selectedCodec == .hevc ? "hevc" : "h264"

        var updated = history.filter { $0 != trimmed }
        updated.insert(trimmed, at: 0)
        if updated.count > 6 {
            updated = Array(updated.prefix(6))
        }
        history = updated
        UserDefaults.standard.set(updated, forKey: historyKey)

        onConnect(trimmed, selectedCodec)
    }

    private func removeHistory(_ server: String) {
        history.removeAll { $0 == server }
        UserDefaults.standard.set(history, forKey: historyKey)
    }
}

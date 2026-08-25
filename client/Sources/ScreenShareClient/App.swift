import SwiftUI
import AppKit

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
        if let window = NSApp.windows.first {
            window.makeKeyAndOrderFront(nil)
        }
    }
}

@main
struct ScreenShareApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) var appDelegate
    @State private var connectionTarget: (host: String, codec: VideoCodecType)?

    init() {
        NSApplication.shared.setActivationPolicy(.regular)
        NSApplication.shared.activate(ignoringOtherApps: true)

        // Se foi passado IP por argumento de linha de comando
        let args = CommandLine.arguments
        if args.count >= 2 {
            let ip = args[1]
            var codec: VideoCodecType = .h264
            if let pos = args.firstIndex(of: "--codec"), pos + 1 < args.count {
                let codecArg = args[pos + 1].lowercased()
                if codecArg == "hevc" || codecArg == "h265" {
                    codec = .hevc
                }
            }
            _connectionTarget = State(initialValue: (host: ip, codec: codec))
        }
    }

    var body: some Scene {
        WindowGroup {
            if let target = connectionTarget {
                StreamView(host: target.host, codec: target.codec) {
                    connectionTarget = nil
                }
                .frame(minWidth: 960, minHeight: 540)
            } else {
                LauncherView { host, codec in
                    connectionTarget = (host: host, codec: codec)
                }
                .fixedSize()
            }
        }
        .windowResizability(connectionTarget != nil ? .automatic : .contentSize)
    }
}

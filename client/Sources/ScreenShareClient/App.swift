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
    @State private var connectionTarget: (host: String, codec: VideoCodecType, resolution: VideoResolutionType)?

    init() {
        NSApplication.shared.setActivationPolicy(.regular)
        NSApplication.shared.activate(ignoringOtherApps: true)

        // Se foi passado IP por argumento de linha de comando
        let args = CommandLine.arguments
        if args.count >= 2 && !args[1].starts(with: "-") {
            let ip = args[1]
            var codec: VideoCodecType = .hevc
            if let pos = args.firstIndex(of: "--codec"), pos + 1 < args.count {
                let codecArg = args[pos + 1].lowercased()
                if codecArg == "h264" || codecArg == "avc" {
                    codec = .h264
                }
            }
            var resolution: VideoResolutionType = .fhd
            if let pos = args.firstIndex(where: { $0 == "--res" || $0 == "--resolution" }), pos + 1 < args.count {
                resolution = VideoResolutionType.fromString(args[pos + 1])
            }
            _connectionTarget = State(initialValue: (host: ip, codec: codec, resolution: resolution))
        }
    }

    var body: some Scene {
        WindowGroup {
            if let target = connectionTarget {
                StreamView(host: target.host, codec: target.codec, resolution: target.resolution) {
                    connectionTarget = nil
                }
                .frame(minWidth: 960, minHeight: 540)
            } else {
                LauncherView { host, codec, resolution in
                    connectionTarget = (host: host, codec: codec, resolution: resolution)
                }
                .fixedSize()
            }
        }
        .windowResizability(connectionTarget != nil ? .automatic : .contentSize)
    }
}

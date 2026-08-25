// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "ScreenShareClient",
    platforms: [
        .macOS(.v13)
    ],
    products: [
        .executable(
            name: "ScreenShareClient",
            targets: ["ScreenShareClient"]
        )
    ],
    dependencies: [],
    targets: [
        .executableTarget(
            name: "ScreenShareClient",
            dependencies: [],
            path: "Sources/ScreenShareClient"
        )
    ]
)

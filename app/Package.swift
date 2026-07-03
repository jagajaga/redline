// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "Redline",
    platforms: [.macOS(.v14)],
    targets: [
        .executableTarget(
            name: "Redline",
            path: "Sources/Redline"
        )
    ]
)

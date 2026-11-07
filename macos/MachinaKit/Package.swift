// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "MachinaKit",
    platforms: [.macOS(.v15)],
    products: [
        .library(name: "MachinaKit", targets: ["MachinaKit"]),
        .executable(name: "Machina", targets: ["Machina"]),
    ],
    targets: [
        .target(name: "MachinaKit"),
        .executableTarget(name: "Machina", dependencies: ["MachinaKit"]),
        .testTarget(name: "MachinaKitTests", dependencies: ["MachinaKit"]),
    ]
)

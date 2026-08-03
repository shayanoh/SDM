// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "SDMKit",
    platforms: [.macOS(.v15)],
    products: [
        .library(name: "SDMCore", targets: ["SDMCore"]),
        .library(name: "SDMEngine", targets: ["SDMEngine"]),
    ],
    targets: [
        .target(name: "SDMCore"),
        .target(name: "SDMEngine", dependencies: ["SDMCore"]),
        .testTarget(name: "SDMCoreTests", dependencies: ["SDMCore"]),
        .testTarget(name: "SDMEngineTests", dependencies: ["SDMEngine"]),
    ]
)

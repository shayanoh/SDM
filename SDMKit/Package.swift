// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "SDMKit",
    platforms: [.macOS(.v15)],
    products: [
        .library(name: "SDMCore", targets: ["SDMCore"]),
        .library(name: "SDMEngine", targets: ["SDMEngine"]),
        .library(name: "SDMGrabber", targets: ["SDMGrabber"]),
    ],
    targets: [
        .target(name: "SDMCore", resources: [.process("Resources")]),
        .target(name: "SDMEngine", dependencies: ["SDMCore"]),
        .target(name: "SDMGrabber", dependencies: ["SDMCore"]),
        .testTarget(name: "SDMCoreTests", dependencies: ["SDMCore"]),
        .testTarget(name: "SDMEngineTests", dependencies: ["SDMEngine"]),
        .testTarget(name: "SDMGrabberTests", dependencies: ["SDMGrabber"]),
    ]
)

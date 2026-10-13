// swift-tools-version: 6.2
import PackageDescription

// The Swift implementation of the core, kept as a baseline for the Rust
// migration experiment. Nothing in the app links this package.
let package = Package(
    name: "SDMKitSwift",
    platforms: [.macOS(.v15)],
    products: [
        .library(name: "SDMCore", targets: ["SDMCore"]),
        .library(name: "SDMEngine", targets: ["SDMEngine"]),
        .library(name: "SDMGrabber", targets: ["SDMGrabber"]),
        .library(name: "SDMResolve", targets: ["SDMResolve"]),
    ],
    targets: [
        .target(name: "SDMCore", resources: [.process("Resources")]),
        .target(
            name: "SDMEngine",
            dependencies: ["SDMCore", "SDMResolve"]
            // Debug-only scheduler/engine/worker event logging (os.log). Comment out
            // the define below to silence it, e.g. before a public release build.
            // swiftSettings: [.define("SDM_ENGINE_LOGGING")]
        ),
        .target(name: "SDMGrabber", dependencies: ["SDMCore", "SDMResolve"]),
        .target(name: "SDMResolve", dependencies: ["SDMCore"]),
        .executableTarget(
            name: "FixtureDump", dependencies: ["SDMCore", "SDMEngine", "SDMResolve"]),
        .testTarget(name: "SDMCoreTests", dependencies: ["SDMCore"]),
        .testTarget(name: "SDMEngineTests", dependencies: ["SDMEngine", "SDMResolve"]),
        .testTarget(name: "SDMGrabberTests", dependencies: ["SDMGrabber", "SDMResolve"]),
        .testTarget(
            name: "SDMResolveTests", dependencies: ["SDMResolve"],
            resources: [.copy("Fixtures")]),
    ]
)

// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "SDMKit",
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
            dependencies: ["SDMCore"],
            // Debug-only scheduler/engine/worker event logging (os.log). Comment out
            // the define below to silence it, e.g. before a public release build.
            // swiftSettings: [.define("SDM_ENGINE_LOGGING")]
        ),
        .target(name: "SDMGrabber", dependencies: ["SDMCore"]),
        .target(name: "SDMResolve", dependencies: ["SDMCore"]),
        .testTarget(name: "SDMCoreTests", dependencies: ["SDMCore"]),
        .testTarget(name: "SDMEngineTests", dependencies: ["SDMEngine"]),
        .testTarget(name: "SDMGrabberTests", dependencies: ["SDMGrabber"]),
        .testTarget(name: "SDMResolveTests", dependencies: ["SDMResolve"]),
    ]
)

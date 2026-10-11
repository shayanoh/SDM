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
        .target(
            name: "SDMCore", dependencies: ["SDMRust"],
            resources: [.process("Resources")]),
        .binaryTarget(name: "SDMRustFFI", path: "Artifacts/SDMRust.xcframework"),
        .target(
            name: "SDMRust",
            dependencies: ["SDMRustFFI"],
            swiftSettings: [.swiftLanguageMode(.v5)],
            linkerSettings: [
                .linkedFramework("Security"),
                .linkedFramework("SystemConfiguration"),
                .linkedFramework("CoreFoundation"),
            ]
        ),
        .target(
            name: "SDMEngine",
            dependencies: ["SDMCore", "SDMResolve"],
            // Debug-only scheduler/engine/worker event logging (os.log). Comment out
            // the define below to silence it, e.g. before a public release build.
            // swiftSettings: [.define("SDM_ENGINE_LOGGING")]
        ),
        .target(name: "SDMGrabber", dependencies: ["SDMCore", "SDMResolve"]),
        .target(name: "SDMResolve", dependencies: ["SDMCore"]),
        .testTarget(name: "SDMRustTests", dependencies: ["SDMRust"]),
        .testTarget(name: "SDMCoreTests", dependencies: ["SDMCore"]),
        .testTarget(name: "SDMEngineTests", dependencies: ["SDMEngine", "SDMResolve"]),
        .testTarget(name: "SDMGrabberTests", dependencies: ["SDMGrabber", "SDMResolve"]),
        .testTarget(
            name: "SDMResolveTests", dependencies: ["SDMResolve"],
            resources: [.copy("Fixtures")]),
    ]
)

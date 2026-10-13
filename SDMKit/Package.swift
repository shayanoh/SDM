// swift-tools-version: 6.2
import PackageDescription

// The four library names the app links are kept, but each target is now a thin
// adapter over `SDMRust`, the UniFFI bindings of the Rust core (`sdm-core/`).
// The original Swift implementation lives in `../legacy/SDMKitSwift` as a
// baseline and is linked into nothing.
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
        .target(name: "SDMCore", dependencies: ["SDMRust"]),
        .target(name: "SDMResolve", dependencies: ["SDMCore", "SDMRust"]),
        .target(name: "SDMEngine", dependencies: ["SDMCore", "SDMResolve", "SDMRust"]),
        .target(name: "SDMGrabber", dependencies: ["SDMCore", "SDMResolve", "SDMRust"]),
        .testTarget(name: "SDMRustTests", dependencies: ["SDMRust"]),
        .testTarget(
            name: "SDMShimTests",
            dependencies: ["SDMCore", "SDMEngine", "SDMGrabber", "SDMResolve"]),
    ]
)

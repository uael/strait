// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "Strait",
    platforms: [.macOS(.v14)],
    targets: [
        // The Rust engine, built by `cargo build --release -p strait-mac`.
        .target(
            name: "StraitCore",
            linkerSettings: [
                .unsafeFlags(["-L../target/release"]),
                .linkedLibrary("strait_mac"),
                // A static library does not carry its frameworks; these are the engine's.
                .linkedFramework("AppKit"),
                .linkedFramework("ApplicationServices"),
                .linkedFramework("AudioToolbox"),
                .linkedFramework("Carbon"),
                .linkedFramework("IOKit"),
            ]
        ),
        .executableTarget(name: "Strait", dependencies: ["StraitCore"]),
    ]
)

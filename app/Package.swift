// swift-tools-version: 6.0
import PackageDescription

// The Rust static library is built by `cargo build -p whoate-ffi [--release]`
// into the workspace target directory next to this package. Swift Build
// applies `.when(configuration:)` linker flags to every configuration, so the
// profile is chosen explicitly: WHOATE_FFI_PROFILE=release for release builds.
let profile = Context.environment["WHOATE_FFI_PROFILE"] ?? "debug"
let library = Context.packageDirectory + "/../target/" + profile
// whoate_engine_new_from_fixture exists only in the debug Rust library.
let fixture = profile == "debug"

let package = Package(
    name: "Whoate",
    platforms: [.macOS(.v14)],
    products: [.executable(name: "Whoate", targets: ["Whoate"])],
    targets: [
        .target(
            name: "CWhoate",
            cSettings: fixture ? [.define("WHOATE_DEBUG")] : [],
            linkerSettings: [.unsafeFlags(["-L\(library)"]), .linkedLibrary("whoate_ffi")]
        ),
        .target(
            name: "WhoateKit",
            dependencies: ["CWhoate"],
            swiftSettings: fixture ? [.define("WHOATE_FIXTURE"), .unsafeFlags(["-Xcc", "-DWHOATE_DEBUG"])] : []
        ),
        .executableTarget(
            name: "Whoate",
            dependencies: ["WhoateKit"],
            swiftSettings: fixture ? [.define("WHOATE_FIXTURE")] : []
        ),
        // `swift test` is unreliable under Command Line Tools; see WhoateChecks/main.swift.
        .executableTarget(
            name: "WhoateChecks",
            dependencies: ["WhoateKit"],
            swiftSettings: fixture ? [.define("WHOATE_FIXTURE")] : []
        ),
    ]
)

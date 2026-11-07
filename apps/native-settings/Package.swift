// swift-tools-version:6.2
import PackageDescription

let package = Package(
    name: "NativeSettingsViews",
    platforms: [.macOS("26.0"), .iOS("26.0")],
    products: [.library(name: "NativeSettingsViews", targets: ["NativeSettingsViews"])],
    targets: [.target(name: "NativeSettingsViews")],
    swiftLanguageModes: [.v6]
)

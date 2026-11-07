// swift-tools-version:6.2
import PackageDescription

let package = Package(
    name: "NativeBrowserViews",
    platforms: [.macOS("26.0"), .iOS("26.0")],
    products: [.library(name: "NativeBrowserViews", targets: ["NativeBrowserViews"])],
    targets: [.target(name: "NativeBrowserViews")],
    swiftLanguageModes: [.v6]
)

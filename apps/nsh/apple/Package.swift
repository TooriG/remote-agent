// swift-tools-version:6.2
import PackageDescription

let package = Package(
    name: "BexNshNative",
    platforms: [.macOS("26.0"), .iOS("26.0")],
    products: [.library(name: "BexNshNative", type: .dynamic, targets: ["BexNshNative"])],
    dependencies: [.package(url: "https://github.com/migueldeicaza/SwiftTerm.git", exact: "1.99.1")],
    targets: [.target(name: "BexNshNative", dependencies: [.product(name: "SwiftTerm", package: "SwiftTerm")])],
    swiftLanguageModes: [.v6]
)

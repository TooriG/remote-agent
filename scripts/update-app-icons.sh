#!/usr/bin/env bash
# Build standard app icons from the logo. Requires macOS.
set -euo pipefail
cd "$(dirname "$0")/.."
source=apps/desktop/assets/logo.png
temporary=$(mktemp -d)
trap 'rm -rf "$temporary"' EXIT
# Standard launcher icons use a white backdrop; the source keeps alpha.
cat > "$temporary/app-icon.swift" <<'SWIFT'
import Foundation
import CoreGraphics
import ImageIO
let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: CommandLine.arguments[1]) as CFURL, nil)!
let image = CGImageSourceCreateImageAtIndex(source, 0, nil)!
let context = CGContext(data: nil, width: 1024, height: 1024, bitsPerComponent: 8,
    bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(),
    bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)!
let bounds = CGRect(x: 0, y: 0, width: 1024, height: 1024)
let color = CommandLine.arguments.count > 3 ? CommandLine.arguments[3] : "release"
let background: CGColor = switch color {
case "preview": CGColor(red: 0.74, green: 0.67, blue: 1, alpha: 1)
case "dev": CGColor(red: 1, green: 0.78, blue: 0.36, alpha: 1)
default: CGColor(gray: 1, alpha: 1)
}
context.setFillColor(background)
context.fill(bounds)
context.draw(image, in: bounds)
let destination = CGImageDestinationCreateWithURL(
    URL(fileURLWithPath: CommandLine.arguments[2]) as CFURL, "public.png" as CFString, 1, nil)!
CGImageDestinationAddImage(destination, context.makeImage()!, nil)
precondition(CGImageDestinationFinalize(destination))
SWIFT
xcrun swiftc "$temporary/app-icon.swift" -o "$temporary/app-icon"
icon=apps/desktop/assets/icon.png
"$temporary/app-icon" "$source" "$icon"
cp "$icon" apps/mobile/iosApp/Bex/Assets.xcassets/AppIcon.appiconset/AppIcon.png
for variant in Preview Dev; do
    app_icon="apps/mobile/iosApp/Bex/Assets.xcassets/AppIcon$variant.appiconset"
    mkdir -p "$app_icon"
    cp apps/mobile/iosApp/Bex/Assets.xcassets/AppIcon.appiconset/Contents.json "$app_icon/Contents.json"
    "$temporary/app-icon" "$source" "$app_icon/AppIcon.png" "$(printf '%s' "$variant" | tr '[:upper:]' '[:lower:]')"
done
sips -z 192 192 "$icon" --out apps/mobile/src/main/res/drawable/agent_icon.png >/dev/null
create_icns() {
    local image=$1 destination=$2 iconset="$temporary/Bex.iconset" size doubled
    mkdir -p "$iconset"
    for size in 16 32 128 256 512; do
        sips -z "$size" "$size" "$image" --out "$iconset/icon_${size}x${size}.png" >/dev/null
        doubled=$((size * 2))
        sips -z "$doubled" "$doubled" "$image" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
    done
    iconutil -c icns "$iconset" -o "$destination"
}
create_icns "$icon" apps/desktop/assets/icon.icns

#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
: "${AGENT_BUILD_NUMBER:?Release build number is required}"
: "${AGENT_RELEASE_SHA:?Release revision is required}"
: "${AGENT_CODE_SIGN_KEYCHAIN:?Dedicated signing keychain is required}"
: "${AGENT_ASC_PRIVATE_KEY_FILE:?App Store Connect key file is required}"
[[ $(git rev-parse HEAD) == "$AGENT_RELEASE_SHA" ]]
git diff --quiet HEAD --
output="$PWD/target/apple-release"
mkdir -p "$output"
# Both products and the bundled Host use the verified source revision.
just build-desktop-macos
bundle="$PWD/target/release/bex.app"
zip="$output/bex-macOS-arm64.zip"
ditto -c -k --sequesterRsrc --keepParent "$bundle" "$zip"
xcrun notarytool submit "$zip" --key "$AGENT_ASC_PRIVATE_KEY_FILE" \
    --key-id "$AGENT_ASC_KEY_ID" --issuer "$AGENT_ASC_ISSUER_ID" \
    --wait --output-format json > "$output/notarization.json"
jq -e -s 'any(.[]; .status == "Accepted")' "$output/notarization.json" >/dev/null
xcrun stapler staple "$bundle"
codesign --verify --deep --strict "$bundle"
spctl --assess --type execute "$bundle"
ditto -c -k --sequesterRsrc --keepParent "$bundle" "$output/notarized.zip"
mv "$output/notarized.zip" "$zip"
(cd "$output" && shasum -a 256 bex-macOS-arm64.zip > bex-macOS-arm64.zip.sha256)
archive="$output/Bex.xcarchive"
auth=(-authenticationKeyPath "$AGENT_ASC_PRIVATE_KEY_FILE" -authenticationKeyID "$AGENT_ASC_KEY_ID" -authenticationKeyIssuerID "$AGENT_ASC_ISSUER_ID")
node scripts/apple-provisioning.mjs
profile_specifier="AGENT_IOS_PROFILE_SPECIFIER=$(jq -er .prefix "$output/provisioning.json") "'$(PRODUCT_BUNDLE_IDENTIFIER)'
# Only the app and extension consume these settings; Swift package build plugins keep their defaults.
just ios-archive "$archive" "$output/DerivedData" "${auth[@]}" \
    "CURRENT_PROJECT_VERSION=$AGENT_BUILD_NUMBER" AGENT_IOS_CODE_SIGN_STYLE=Manual \
    'AGENT_IOS_CODE_SIGN_IDENTITY=Apple Distribution' "$profile_specifier" \
    "OTHER_CODE_SIGN_FLAGS=--keychain $AGENT_CODE_SIGN_KEYCHAIN"
app="$archive/Products/Applications/Bex.app"
codesign --verify --deep --strict "$app"
[[ $(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$app/Info.plist") == "$AGENT_BUILD_NUMBER" ]]
[[ $(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Info.plist") == dev.remoteagent.mobile.ios ]]
# Prove distribution signing locally before sending the archive to Apple.
for destination in export upload; do
    if [[ $destination == upload ]]; then touch "$output/upload-start"; fi
    options="$output/export-$destination.plist"
    jq --arg destination "$destination" '{method:"app-store-connect",destination:$destination,
        teamID:"K65K9J8686",signingStyle:"manual",signingCertificate:"Apple Distribution",
        provisioningProfiles:.profiles,manageAppVersionAndBuildNumber:false,uploadSymbols:true}' \
        "$output/provisioning.json" | plutil -convert xml1 -o "$options" -
    xcodebuild -exportArchive -archivePath "$archive" -exportPath "$output/$destination" \
        -exportOptionsPlist "$options" -allowProvisioningUpdates "${auth[@]}" \
        "OTHER_CODE_SIGN_FLAGS=--keychain $AGENT_CODE_SIGN_KEYCHAIN"
done
node scripts/apple-release.mjs record-delivery "$(/usr/bin/getconf DARWIN_USER_TEMP_DIR)" "$output/upload-start"

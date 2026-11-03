{
  description = "Remote Agent development environment";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.rust-overlay.url = "github:oxalica/rust-overlay";
  inputs.rust-overlay.inputs.nixpkgs.follows = "nixpkgs";

  outputs = { nixpkgs, rust-overlay, ... }:
    let
      supportedSystems = [ "aarch64-darwin" "aarch64-linux" "x86_64-linux" ];
      forEachSystem = function:
        nixpkgs.lib.genAttrs supportedSystems (system:
          function (import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
            config = {
              allowUnfree = true;
              android_sdk.accept_license = true;
            };
          }));
    in
    {
      packages = forEachSystem (pkgs: {
        agent-peer = pkgs.callPackage ./tools/agent-peer/package.nix { };
        kani = pkgs.callPackage ./tools/kani/package.nix { };
        kache = pkgs.callPackage ./tools/kache/package.nix { };
        maestro = pkgs.callPackage ./tools/maestro/package.nix { };
      } // pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isDarwin {
        node-runtime = pkgs.callPackage ./tools/node-runtime/package.nix { };
      });
      devShells = forEachSystem (pkgs:
        let
          # Queue validation is not released yet: https://github.com/rhysd/actionlint/pull/654
          workflowLinter = pkgs.actionlint.overrideAttrs (old: {
            patches = (old.patches or [ ]) ++ [ (pkgs.fetchpatch {
              url = "https://github.com/rhysd/actionlint/commit/644076a59742c2d1540ebd4686eab3c308f0e562.patch";
              hash = "sha256-Lpd7CTkLbzgvAZV2H8G4GTUoPNDCK7PiW3A40+cquUk=";
            }) ];
            postCheck = (old.postCheck or "") + ''
              go test . -run '^TestLinterLint(OK|Error)$'
            '';
          });
          buildRustToolchain = pkgs.rust-bin.stable.latest.minimal;
          hostRustToolchain = buildRustToolchain.override {
            extensions = [ "clippy" "rustfmt" "rust-analyzer" "rust-src" ];
          };
          mobileRustToolchain = hostRustToolchain.override {
            targets = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
              "aarch64-apple-ios"
              "aarch64-apple-ios-sim"
            ] ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [
              "aarch64-linux-android"
              "x86_64-linux-android"
            ];
          };
          cccc = pkgs.callPackage ./tools/cccc/package.nix { };
          kache = pkgs.callPackage ./tools/kache/package.nix { };
          nodeRuntime = pkgs.callPackage ./tools/node-runtime/package.nix { };
          kacheHook = ''
            # CI already restores Cargo outputs.
            if [ -z "''${CI:-}" ]; then
              export RUSTC_WRAPPER="${kache}/bin/kache"
              if [ -z "''${KACHE_CACHE_DIR:-}" ]; then
                agent_kache_build_root=$(git config --path --get bex.buildRoot 2>/dev/null || true)
                if [ -n "$agent_kache_build_root" ]; then
                  if [ "''${agent_kache_build_root#/}" = "$agent_kache_build_root" ] || [ ! -d "$agent_kache_build_root" ]; then
                    printf 'Build storage is unavailable: %s\n' "$agent_kache_build_root" >&2
                    exit 1
                  fi
                  # Keep the shared compiler cache on the build volume for CoW restores.
                  export KACHE_CACHE_DIR="$agent_kache_build_root/.kache"
                else
                  export KACHE_CACHE_DIR="$HOME/${if pkgs.stdenv.hostPlatform.isDarwin then "Library/Caches/kache" else ".cache/kache"}"
                fi
                unset agent_kache_build_root
              fi
            fi
          '';
          androidSdk = (pkgs.androidenv.composeAndroidPackages {
            platformVersions = [ "37.0" ];
            buildToolsVersions = [ "36.0.0" ];
            includeNDK = true;
            includeEmulator = false;
            includeSystemImages = false;
          }).androidsdk;
          androidTestSdk = (pkgs.androidenv.composeAndroidPackages {
            platformVersions = [ "37.0" ];
            buildToolsVersions = [ "36.0.0" ];
            includeEmulator = true;
            includeSystemImages = true;
            systemImageTypes = [ "google_apis" ];
            abiVersions = [ (if pkgs.stdenv.hostPlatform.isAarch64 then "arm64-v8a" else "x86_64") ];
          }).androidsdk;
          androidBuildEnvironment = {
            JAVA_HOME = pkgs.jdk21.home;
            ANDROID_HOME = "${androidSdk}/libexec/android-sdk";
            ANDROID_SDK_ROOT = "${androidSdk}/libexec/android-sdk";
            ANDROID_NDK_HOME = "${androidSdk}/libexec/android-sdk/ndk-bundle";
          };
          nativeBuildTools = with pkgs; [ pkg-config cmake clang zig ] ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [ pkgs.patchelf ];
          # The desktop, Host and their tests link these on Linux.
          linuxDesktopLibraries = with pkgs; [
            lsof
            alsa-lib fontconfig freetype libxkbcommon wayland libGL vulkan-loader
            libxcb libX11 libXcursor libXi libXrandr
            openssl procps
          ];
          linuxDesktopLibraryPath = pkgs.lib.makeLibraryPath [ pkgs.vulkan-loader pkgs.libGL pkgs.libxkbcommon pkgs.wayland ];
          runtimeShellHook = ''
            # Keep child processes and interactive sessions on the pinned shell.
            export SHELL="${pkgs.zsh}/bin/zsh"
            ${pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isDarwin ''
              # Use the kernel-matched process inspector on macOS.
              export PATH="/usr/sbin:$PATH"
            ''}
            case $- in *i*) exec "$SHELL" ;; esac
          '';
          appleSdkHook = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isDarwin ''
            # The Nix compiler setup overwrites DEVELOPER_DIR with its own SDK.
            if [ -d "''${AGENT_XCODE_DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}" ]; then
              export DEVELOPER_DIR="''${AGENT_XCODE_DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
              unset SDKROOT
              # Xcode expects clang, not Nix's LD override with clang-only flags.
              unset LD
            fi
          '';
          mkDevShell = attrs: pkgs.mkShell (attrs // {
            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
            packages = [ pkgs.zsh ] ++ (attrs.packages or [ ]);
            shellHook = (attrs.shellHook or "") + runtimeShellHook;
          });
        in
        {
          repository = pkgs.mkShellNoCC {
            packages = with pkgs; [ bash git just nodejs perl powershell workflowLinter cccc ];
          };
          kani = mkDevShell {
            packages = with pkgs; [
              (callPackage ./tools/kani/package.nix { })
              hostRustToolchain just git clang pkg-config
            ];
          };
          native = mkDevShell {
            AGENT_NODE_RUNTIME = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isDarwin "${nodeRuntime}";
            RUST_TOOLCHAIN_VERSION = hostRustToolchain.version;
            NEXTEST_VERSION = pkgs.cargo-nextest.version;
            packages = with pkgs; [ hostRustToolchain kache cccc cargo-mutants cargo-nextest just jq git perl ] ++ nativeBuildTools
              ++ [ workflowLinter nodejs powershell ] ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux linuxDesktopLibraries;
            LD_LIBRARY_PATH = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux linuxDesktopLibraryPath;
            shellHook = kacheHook + appleSdkHook;
          };
          ios-test = pkgs.mkShellNoCC {
            packages = with pkgs; [ just gitMinimal nodejs zsh ];
            shellHook = appleSdkHook + runtimeShellHook;
          };
          android-test = pkgs.mkShell {
            packages = [ androidTestSdk pkgs.jdk21 buildRustToolchain pkgs.coreutils pkgs.zsh ];
            JAVA_HOME = pkgs.jdk21.home;
            ANDROID_HOME = "${androidTestSdk}/libexec/android-sdk";
            ANDROID_SDK_ROOT = "${androidTestSdk}/libexec/android-sdk";
            shellHook = runtimeShellHook;
          };
          default = mkDevShell ({
            AGENT_NODE_RUNTIME = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isDarwin "${nodeRuntime}";
            packages = with pkgs; [
              just
              jq
              shellcheck
              workflowLinter
              nodejs
              perl
              powershell
              mobileRustToolchain
              kache
              cccc
              cargo-mutants
              cargo-nextest
            ] ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux (nativeBuildTools ++ linuxDesktopLibraries ++ [
              # Host worktree management needs a newer Git than many distributions ship.
              git
              gradle
              jdk21
              cargo-ndk
              kotlin-language-server
              androidSdk
            ]) ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
              pkgs.zig
              pkgs.swiftlint
              pkgs.swiftformat
            ];

            shellHook = kacheHook + appleSdkHook;
          } // pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux (androidBuildEnvironment // {
            LD_LIBRARY_PATH = linuxDesktopLibraryPath;
          }));
        } // pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
          host-build = mkDevShell {
            packages = with pkgs; [
              buildRustToolchain kache just jq gitMinimal nodejs pkg-config cmake clang zig
            ];
            shellHook = kacheHook;
          };
          android-build = pkgs.mkShell (androidBuildEnvironment // {
            packages = with pkgs; [
              (buildRustToolchain.override {
                targets = [ "aarch64-linux-android" "x86_64-linux-android" ];
              })
              kache zsh just jq gitMinimal nodejs cargo-ndk jdk21 androidSdk
            ];
            shellHook = kacheHook + runtimeShellHook;
          });
        } // pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isDarwin {
          ios-build = mkDevShell {
            packages = with pkgs; [
              (buildRustToolchain.override {
                targets = [ "aarch64-apple-ios-sim" ];
              })
              kache just jq gitMinimal nodejs zig swiftlint swiftformat
            ];
            shellHook = kacheHook + appleSdkHook;
          };
        });
    };
}

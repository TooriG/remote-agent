{ lib, callPackage }:

let
  version = "1.0.0";
  mkReleaseBinary = callPackage ../github-release-binary.nix { };
in
mkReleaseBinary {
  pname = "kache";
  inherit version;
  url = target: "https://github.com/kunobi-ninja/kache/releases/download/v${version}/kache-${target}.tar.gz";
  hashes = {
    aarch64-apple-darwin = "6d1be0079d0689a85fa04b7fed7eaa94f7e08259cafb8bd361a5c25c4c98c4a3";
    aarch64-unknown-linux-musl = "88abd848be7d300d4e30b8510ebdc45990dae3f96b6591e3498209639cf34701";
    x86_64-unknown-linux-musl = "756e9701a6afb8354fd8b1d76164e13272d320354d84f01197e57d8a3b4be397";
  };
  meta = {
    description = "Content-addressed compiler cache for Rust and C/C++";
    homepage = "https://github.com/kunobi-ninja/kache";
    license = lib.licenses.asl20;
  };
}

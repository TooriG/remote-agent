{ lib, callPackage }:

let
  version = "1.8.5";
  mkReleaseBinary = callPackage ../github-release-binary.nix { };
in
mkReleaseBinary {
  pname = "cccc";
  inherit version;
  url = target: "https://github.com/moznion/cccc/releases/download/v${version}/cccc-v${version}-${target}.tar.gz";
  hashes = {
    aarch64-apple-darwin = "486ded852eaa3483956acde058e32b3616a4883c84ba1f424852680e61963801";
    aarch64-unknown-linux-musl = "62004c8f7bfd5c452a5e0afe54248919d7f3156ab707c24d33715566ffc3182e";
    x86_64-unknown-linux-musl = "7419ea8d8828c3d405694be400da1b21cdb854e9af1c10110d5d970bc1700094";
  };
  meta = {
    description = "Cognitive and cyclomatic complexity analyzer";
    homepage = "https://github.com/moznion/cccc";
    license = lib.licenses.mit;
  };
}

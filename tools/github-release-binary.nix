{ lib, stdenvNoCC, fetchurl }:

{ pname, version, url, hashes, meta }:
let
  target = if stdenvNoCC.hostPlatform.isLinux
    then "${stdenvNoCC.hostPlatform.parsed.cpu.name}-unknown-linux-musl"
    else stdenvNoCC.hostPlatform.rust.rustcTarget;
in
stdenvNoCC.mkDerivation {
  inherit pname version;
  src = fetchurl {
    url = url target;
    sha256 = hashes.${target};
  };
  sourceRoot = ".";
  dontBuild = true;
  dontStrip = true;
  installPhase = ''
    runHook preInstall
    mkdir -p "$out/bin"
    install -m755 ${pname} "$out/bin/${pname}"
    runHook postInstall
  '';
  doInstallCheck = true;
  installCheckPhase = ''
    runHook preInstallCheck
    "$out/bin/${pname}" --version
    runHook postInstallCheck
  '';
  meta = {
    platforms = map (target: (lib.systems.elaborate target).system) (builtins.attrNames hashes);
    mainProgram = pname;
  } // meta;
}

{
  stdenv,
  fetchzip,
  writeTextFile,

  version,
  aarch64Hash,
  x86_64Hash,
  ...
}:
let
  vectorConfiguration = writeTextFile {
    name = "config.toml";
    executable = false;

    text = builtins.readFile ./vector.yaml;
  };
  vectorEntry = writeTextFile {
    name = "vector";
    executable = true;
    text = ''
      #!/usr/bin/env bash

      set -P

      NIX_BIN_DIR="$(cd "$(dirname "''${BASH_SOURCE[0]}")" && pwd)"
      NIX_PACKAGE_ROOT=$(dirname $NIX_BIN_DIR)

      # Important 'exec' needs to be there, otherwise vector doesn't inherit fd's 
      # the correct way, which means that stdio source could break.
      exec "$NIX_BIN_DIR/.vector-wrapped" --config-yaml $NIX_PACKAGE_ROOT/config.yaml
    '';
  };
in
stdenv.mkDerivation {
  dontPatchShebangs = true;
  name = "vers-vector";
  src = fetchzip {
    url = "https://github.com/vectordotdev/vector/releases/download/v${version}/vector-${version}-${stdenv.buildPlatform.parsed.cpu.name}-unknown-linux-musl.tar.gz";
    # 'else' path should only run for x86_64 based systems. As currently this
    # only supports x86_64 and aarch64 linux this is fine.
    hash = if stdenv.buildPlatform.parsed.cpu.name == "aarch64" then aarch64Hash else x86_64Hash;
  };
  installPhase = ''
    mkdir -p $out/bin
    cp ./bin/vector $out/bin/.vector-wrapped
    cp -r ${vectorConfiguration} $out/config.yaml
    cp ${vectorEntry} $out/bin/vector
  '';

  doInstallCheck = true;
  installCheckPhase = ''
    echo "Checking that vector is statically linked..."
    if ! file $out/bin/.vector-wrapped | grep -qE "(statically linked|static-pie linked)"; then
      echo "ERROR: vector is not statically linked!"
      file $out/bin/.vector-wrapped
      exit 1
    fi

    echo "✓ Vector binary is statically linked"
  '';

  meta = {
    platforms = [
      "x86_64-linux"
      "aarch64-linux"
    ];
  };
}

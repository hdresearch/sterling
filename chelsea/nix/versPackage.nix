{
  stdenv,
  rustPlatform,
  rustToolchain,
  lib,
  config,
  pkgsStatic,
  pkg-config-unwrapped,
  zlib,
  pkgs,
  self,
  ...
}:
rustPlatform.buildRustPackage {
  pname = "vers-output";
  version = "1.0.0";

  cargoLock.lockFile = ../Cargo.lock;
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../VERSION
      ../Cargo.toml
      ../Cargo.lock
      ../crates
      ../config
    ];
  };

  doCheck = false;
  buildPhase = ''
    # Gets picked up by crate 'workspace_build'
    # nix doesn't include the .git path even if specified so this is a 
    # workaround.
    export VERS_GIT_HASH=${self.rev or "dirty"}
    cargo build --release \
      -p orchestrator --bin orchestrator \
      -p chelsea --bin chelsea \
      -p proxy --bin proxy \
      -p chelsea-agent --bin chelsea-agent \
      --target ${stdenv.buildPlatform.parsed.cpu.name}-unknown-linux-musl
  '';
  nativeBuildInputs = [
    rustToolchain
    pkg-config-unwrapped
    pkgsStatic.stdenv.cc
    pkgsStatic.sqlite.out
    zlib.static
    pkgs.git
  ];

  PKG_CONFIG_PATH = "${pkgsStatic.openssl.dev}/lib/pkgconfig:${pkgsStatic.sqlite.dev}/lib/pkgconfig";
  "PKG_CONFIG_ALLOW_CROSS_${stdenv.buildPlatform.parsed.cpu.name}-unknown-linux-musl" = "1";

  installPhase = ''
    mkdir -p $out/bin

    cp target/${stdenv.buildPlatform.parsed.cpu.name}-unknown-linux-musl/release/{chelsea,proxy,orchestrator,chelsea-agent} $out/bin

    # Setup config
    cp -r ${config}/* $out/bin
  '';

  meta.platforms = [
    "x86_64-linux"
    "aarch64-linux"
  ];
}

{
  # Our rust toolchain (built from ../rust-toolchain.toml
  stdenv,
  rustToolchain,

  # Methods on pkgs (pkgs.*)
  rustPlatform,
  fetchFromGitHub,

  # nixpkgs derivations on pkgs (pkgs.*)
  rust-bindgen,
  pkgsStatic,
  cmake,
  llvmPackages,
  linuxHeaders,

  # Arguments we provide for this derivation
  version,
  hash,
  cargoHash,
  ...
}:
rustPlatform.buildRustPackage {
  pname = "firecracker";
  inherit version;

  src = fetchFromGitHub {
    owner = "firecracker-microvm";
    repo = "firecracker";
    rev = "v${version}";
    inherit hash;
  };

  patches = [
    ./patches/prevent_badsyscall44.patch

    ../../kernel/patches/enable_modules_1.patch
    ../../kernel/patches/enable_modules_2.patch

    # This one is problematic
    # ../../kernel/patches/enable_prefault_1.patch
  ];

  inherit cargoHash;

  # Don't run firecracker tests during build process.
  doCheck = false;

  nativeBuildInputs = [
    rustToolchain
    cmake
    pkgsStatic.libseccomp.lib
    rust-bindgen
    rustPlatform.bindgenHook
    pkgsStatic.stdenv.cc
  ];

  AWS_LC_SYS_EXTERNAL_BINDGEN = "true";

  # to help aws-lc-sys on the way.
  LIBCLANG_PATH = "${llvmPackages.libclang.lib}/lib";

  CC = "${stdenv.buildPlatform.parsed.cpu.name}-unknown-linux-musl-gcc";

  # to help aws-lc-sys on the way.
  BINDGEN_EXTRA_CLANG_ARGS = [
    "-I${linuxHeaders}/include"
  ];

  RUSTFLAGS = "-L ${pkgsStatic.libseccomp.lib}/lib -A warnings";

  buildPhase = ''
    runHook preBuild

    # Why '--offline'? nix's buildPhase doesn't have internet access to make builds more reproducable.
    # Why musl target? Statically linked binaries are a requirement.
    cargo build \
      -j $NIX_BUILD_CORES \
      --offline \
      --target ${stdenv.buildPlatform.parsed.cpu.name}-unknown-linux-musl \
      --release \
      -p firecracker \
      -p jailer
  '';

  installPhase = ''
    mkdir -p $out/bin
    cp ./build/cargo_target/${stdenv.buildPlatform.parsed.cpu.name}-unknown-linux-musl/release/{firecracker,jailer} $out/bin
  '';

  doInstallCheck = true;
  installCheckPhase = ''
    echo "Checking that firecracker is statically linked..."
    if ! file $out/bin/firecracker | grep -qE "(statically linked|static-pie linked)"; then
      echo "ERROR: firecracker is not statically linked!"
      file $out/bin/firecracker
      exit 1
    fi

    echo "Checking that jailer is statically linked..."
    if ! file $out/bin/jailer | grep -qE "(statically linked|static-pie linked)"; then
      echo "ERROR: jailer is not statically linked!"
      file $out/bin/jailer
      exit 1
    fi

    echo "✓ All binaries are statically linked"
  '';

  meta = {
    platforms = [
      "x86_64-linux"
      "aarch64-linux"
    ];
  };
}

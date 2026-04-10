# NOTE: Chelsea nix package will need 'rbd' command pre-installed.
# @vincent-thomas had trouble including this into the chelsea package.
#
# HOW TO:
# - Build all three binaries?
#   `nix build .` (non-self-contained).
#
# - Build one of three binaries (proxy,orchestrator,chelsea)?
#   `nix build .#[binary-name]` (Look into flake.nix to the 'packages' definition.
#
#
# Short glossary (read in order):
# - 'flake':
#   A reproducible Nix project that defines input (via lockfile) and outputs.
#
# - 'packages':
#   A kind of flake output that can be installed/built with 'nix build <path-to-flake>#[pkg-name]'.
#   A package can be defined as key 'default', then only
#   'nix build <path-to-flake.nix>' is required. They are defined per arch,
#   then package name. We have helpers 'github.com/numtide/flake-utils', that
#   makes this easier for us.
#
# - 'nixpkgs':
#   Nix package repository, the standard library of packages for Nix.
#   'nixpkgs' is just a flake with a lot of packages defined in it's output.
#
# - 'nativeBuildInputs':
#   Build-time dependencies (compilers, build tools) that are only available
#   when building.
#
# - 'buildInputs':
#   Runtime dependencies needed after building (e.g., dynamically linked libraries)
#
# - '*Phase':
#   Build lifecycle hooks (buildPhase, installPhase, postFixup, etc.) that
#   execute during package creation
#
# - 'derivation/mkDerivation':
#   Functions that define how to build a package from source to output.
#
# - 'devShells':
#   Development environments with tools/deps available via 'nix develop'

# What is 'with pkgs; [ * ]' ????: it's a shorthand for [ pkgs.* ]

{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # On "chelsea/production" ref should at all times be "main".
    configuration.url = "git+ssh://git@github.com/hdresearch/configuration";
    configuration.flake = false;
  };

  outputs =
    inputs@{
      self,
      nixpkgs,
      flake-utils,
      rust-overlay,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };

        inherit (nixpkgs) lib;
        inherit (pkgs) stdenv;

        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

        # https://nixos.org/guides/nix-pills/13-callpackage-design-pattern.html
        callPackage =
          path: overrides:
          let
            f = import path;
          in
          f ((builtins.intersectAttrs (builtins.functionArgs f) pkgs // lib) // overrides);

        # 2. The package derivation uses this store path as its source.
        config = stdenv.mkDerivation {
          name = "vers-config";
          src = inputs.configuration;

          installPhase = ''
            mkdir -p $out
            cp -r ./config $out/
          '';
        };
      in
      {
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = [
            rustToolchain

            pkgs.pkgsStatic.stdenv.cc
            pkgs.pkg-config-unwrapped

            # libslite.a
            pkgs.pkgsStatic.sqlite.out
          ];

          # env vars.
          PKG_CONFIG_PATH = "${pkgs.pkgsStatic.openssl.dev}/lib/pkgconfig:${pkgs.pkgsStatic.sqlite.dev}/lib/pkgconfig";
          "PKG_CONFIG_ALLOW_CROSS_${stdenv.buildPlatform.parsed.cpu.name}-unknown-linux-musl" = "1";
          RUST_BACKTRACE = "full";
        };

        packages = rec {
          inherit config;

          firecracker = callPackage ./nix/firecracker {
            inherit rustToolchain;

            version = "1.13.1";
            hash = "sha256-ZrIvz5hmP0d8ADF723Z+lOP9hi5nYbi6WUtV4wTp73U=";
            cargoHash = "sha256-BjaNUYZRPKJKjiQWMUyoBIdD2zsNqZX37CPzdwb+lCE=";
          };

          vector = callPackage ./nix/vector {
            version = "0.52.0";
            x86_64Hash = "sha256-5EZZ6Hfg4vI/G8saQomsy4KfzHiIDyr4S3KFuYi6e/8=";
            aarch64Hash = "sha256-pqORGk2KqkfpdQTpqemkTEotaGtNjllYpQB+b33+ySE=";
          };

          # 'default' is a package which includes all of our binaries
          # and the configuration from github:hdresearch/configuration.
          default = callPackage ./nix/versPackage.nix {
            inherit config rustToolchain self;
            inherit (pkgs) rustPlatform;
          };

          proxy = stdenv.mkDerivation {
            name = "proxy";
            src = default;
            installPhase = ''
              mkdir -p $out/bin
              cp ./bin/config/* $out/bin/
              cp ./bin/proxy $out/bin/
            '';
          };

          orchestrator = stdenv.mkDerivation {
            name = "orchestrator";
            src = default;
            installPhase = ''
              mkdir -p $out/bin
              cp ./bin/config/* $out/bin/
              cp ./bin/orchestrator $out/bin/
            '';
          };

          # How to run production nix-built chelsea (with vector).
          # $ nix build .#chelsea
          # $ ./result/bin/chelsea | ./result/bin-deps/vector/bin/vector
          chelsea =
            let
              chelseaEntry = pkgs.writeTextFile {
                name = "chelsea";
                executable = true;
                text = ''
                  #!/usr/bin/env bash

                  # Follow symlinks
                  set -P

                  NIX_BIN_DIR="$(cd "$(dirname "''${BASH_SOURCE[0]}")" && pwd)"
                  NIX_PACKAGE_ROOT=$(dirname $NIX_BIN_DIR)

                  # Remove '$PATH:' when we are completely isolated. Currently we
                  # are not able to isolate 'rbd'.
                  export PATH=$PATH:$NIX_PACKAGE_ROOT/bin-deps
                  export FIRECRACKER_BIN_PATH="$NIX_PACKAGE_ROOT/bin-deps/firecracker"

                  exec "$NIX_BIN_DIR/.chelsea-wrapped"
                '';
              };
            in
            stdenv.mkDerivation {
              dontPatchShebangs = true;
              name = "chelsea";
              src = default;
              installPhase = ''
                mkdir -p $out/bin
                mkdir -p $out/bin-deps
                cp -r ./bin/config/* $out/bin/
                cp ./bin/chelsea $out/bin/.chelsea-wrapped
                cp ./bin/chelsea-agent $out/bin/chelsea-agent

                # Loading our dependencies here.
                cp ${pkgs.pkgsStatic.nftables}/bin/nft $out/bin-deps/
                cp ${pkgs.pkgsStatic.iproute2}/bin/ip $out/bin-deps/
                cp ${pkgs.pkgsStatic.iptables}/bin/{iptables,ip6tables} $out/bin-deps/
                cp ${pkgs.pkgsStatic.procps}/bin/sysctl $out/bin-deps/
                cp ${pkgs.pkgsStatic.openssh}/bin/ssh $out/bin-deps/
                cp -r ${vector} $out/bin-deps/vector

                cp ${firecracker}/bin/firecracker $out/bin-deps/
                cp ${firecracker}/bin/jailer $out/bin-deps/
                cp ${chelseaEntry} $out/bin/chelsea
              '';
            };
        };
      }
    );
}

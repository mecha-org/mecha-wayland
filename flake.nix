{
  description = "mecha-wayland";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url       = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, fenix, crane, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs      = import nixpkgs { inherit system; };
        pkgsCross = pkgs.pkgsCross.aarch64-multiplatform;
        lib       = pkgs.lib;
        fn        = fenix.packages.${system};

        # build-std (.cargo/config.toml) requires rust-src at build time.
        # devToolchain adds it to devShell for rust-analyzer as well.
        buildToolchain = fn.combine [
          fn.latest.cargo
          fn.latest.rustc
          fn.latest.rust-src
          fn.latest.clippy
          fn.latest.rustfmt
          fn.targets.aarch64-unknown-linux-gnu.latest.rust-std
        ];

        devToolchain = buildToolchain;

        craneLib = (crane.mkLib pkgs).overrideToolchain buildToolchain;

        # atlas.toml, .xml (Wayland protocols), and /assets/ are read by build.rs scripts
        src = lib.cleanSourceWith {
          src    = ./.;
          filter = path: type:
            (craneLib.filterCargoSources path type)
            || lib.hasSuffix "atlas.toml" path
            || lib.hasSuffix ".xml"       path
            || lib.hasInfix  "/assets/"   path;
        };

        # build-std requires vendoring the stdlib's own deps alongside the
        # project's deps.  crane's vendorMultipleCargoDeps merges both lockfiles.
        cargoVendorDir = craneLib.vendorMultipleCargoDeps {
          cargoLockList = [
            ./Cargo.lock
            "${buildToolchain}/lib/rustlib/src/rust/library/Cargo.lock"
          ];
        };

        crossArgs = {
          inherit src cargoVendorDir;
          strictDeps = true;

          # Tests are unrunnable with build-std=panic_abort (panic_unwind conflicts).
          doCheck = false;

          CARGO_BUILD_TARGET = "aarch64-unknown-linux-gnu";

          CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER =
            "${pkgsCross.stdenv.cc}/bin/${pkgsCross.stdenv.cc.targetPrefix}cc";

          CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS =
            "-L${pkgsCross.mesa}/lib -L${pkgsCross.libdrm}/lib -C target-cpu=cortex-a53";

          PKG_CONFIG_ALLOW_CROSS = "1";
          PKG_CONFIG_PATH = lib.makeSearchPathOutput "dev" "lib/pkgconfig"
            (with pkgsCross; [ mesa libdrm ]);

          nativeBuildInputs = with pkgs; [ pkg-config clang pkgsCross.stdenv.cc ];
          buildInputs       = with pkgsCross; [ mesa libdrm ];
        };

        launcherAarch64 = craneLib.buildPackage (crossArgs // {
          pname          = "launcher";
          version        = "0.1.0";
          cargoExtraArgs = "--package launcher";

          nativeBuildInputs = (crossArgs.nativeBuildInputs or []) ++ [ pkgs.patchelf ];

          postInstall = ''
            patchelf \
              --set-interpreter /lib/ld-linux-aarch64.so.1 \
              --set-rpath /usr/lib/aarch64-linux-gnu:/usr/lib \
              $out/bin/launcher
          '';
        });

      in {
        packages = {
          launcher-aarch64 = launcherAarch64;
          default          = launcherAarch64;
        };

        devShells.default = pkgs.mkShell {
          nativeBuildInputs = [
            devToolchain
            pkgs.pkg-config
            pkgs.mold
            pkgs.clang
            pkgs.rust-analyzer
            pkgs.cargo-watch
          ];

          buildInputs = with pkgs; [ mesa libdrm wayland wayland-protocols libxkbcommon ];

          RUST_SRC_PATH  = "${devToolchain}/lib/rustlib/src/rust/library";
          RUST_BACKTRACE = "1";
        };
      }
    );
}

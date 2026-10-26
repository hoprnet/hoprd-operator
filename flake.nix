# flake.nix - HOPR hoprd-operator Nix flake configuration
#
# Builds the operator, its Docker images and the quality checks that the shared
# hopr-workflows (tests.yaml, build-docker.yaml) run in CI. It uses the HOPR
# nix-lib for the Rust builders, the Docker image builder and the dev shell.
{
  description = "HOPR hoprd-operator - Kubernetes operator that manages hoprd nodes";

  inputs = {
    flake-parts.url = "github:hercules-ci/flake-parts";
    nixpkgs.url = "github:NixOS/nixpkgs/release-26.05";

    # HOPR Nix Library (reusable Rust builders and Docker images)
    nix-lib.url = "github:hoprnet/nix-lib/v1.3.0";

    # Rust build system
    crane.url = "github:ipetkov/crane";
    rust-overlay.url = "github:oxalica/rust-overlay";

    flake-root.url = "github:srid/flake-root";

    # Input dependency optimization
    flake-parts.inputs.nixpkgs-lib.follows = "nixpkgs";
    nix-lib.inputs.nixpkgs.follows = "nixpkgs";
    nix-lib.inputs.crane.follows = "crane";
    nix-lib.inputs.rust-overlay.follows = "rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-parts,
      nix-lib,
      rust-overlay,
      ...
    }@inputs:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [
        inputs.nix-lib.flakeModules.default
        inputs.flake-root.flakeModule
      ];

      perSystem =
        {
          config,
          lib,
          system,
          ...
        }:
        let
          rev = toString (self.shortRev or self.dirtyShortRev);
          buildVersionEnv = builtins.getEnv "BUILD_VERSION";
          buildVersion = if buildVersionEnv == "" then null else buildVersionEnv;

          fs = lib.fileset;
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };

          nixLib = nix-lib.lib.${system};

          # The crate sources, filtered so that unrelated changes do not invalidate the build cache
          sources = {
            main = nixLib.mkSrc {
              inherit fs;
              root = ./.;
            };
            # The unit tests read the sample configuration of the operator
            test = nixLib.mkTestSrc {
              inherit fs;
              root = ./.;
              extraExtensions = [ "yaml" ];
              extraFiles = [ ./test-data ];
            };
            deps = nixLib.mkDepsSrc {
              inherit fs;
              root = ./.;
            };
          };

          builders = nixLib.mkRustBuilders {
            rustToolchainFile = ./rust-toolchain.toml;
          };

          mkBuildArgs =
            { src }:
            {
              inherit
                src
                rev
                buildVersion
                ;
              depsSrc = sources.deps;
              cargoToml = ./Cargo.toml;
            };

          # Static binary for Linux, which is what the Docker image ships.
          # `--locked` is deliberately not used: the shared build-docker workflow rewrites the package version in
          # Cargo.toml (commit/pr suffix) without refreshing Cargo.lock, so `--locked` would fail every CI build.
          # Dependencies stay pinned, as Nix vendors them from the Cargo.lock.
          mkBinary = platform: builders.${platform}.callPackage nixLib.mkRustPackage (mkBuildArgs { src = sources.main; });

          binaries = {
            hoprd-operator-x86_64-linux = mkBinary "x86_64-linux";
          };

          # Docker images only need Linux packages, even when building on macOS
          platformPkgsMap = {
            "x86_64-linux" = import nixpkgs { system = "x86_64-linux"; };
          };

          mkDocker =
            targetPlatform:
            let
              binary = binaries."hoprd-operator-${targetPlatform}";
            in
            nixLib.mkDockerImage {
              name = "hoprd-operator";
              Entrypoint = [ "${binary}/bin/hoprd_operator" ];
              pkgsLinux = platformPkgsMap.${targetPlatform};
              env = [
                "OPERATOR_ENVIRONMENT=production"
                "SSL_CERT_FILE=${platformPkgsMap.${targetPlatform}.cacert}/etc/ssl/certs/ca-bundle.crt"
              ];
              extraContents = [ binary ];
            };

          dockerImages = {
            docker-hoprd-operator-x86_64-linux = mkDocker "x86_64-linux";
          };

          # Unit tests, run through cargo-llvm-cov so they also produce the LCOV report.
          # The crate is binary-only, so the plain `runTests` mode of nix-lib (which builds the test
          # dependencies with `--lib`) cannot be used.
          hoprdOperatorTest = builders.localCoverage.callPackage nixLib.mkRustPackage (
            (mkBuildArgs { src = sources.test; })
            // {
              runCoverage = true;
              cargoLlvmCovCommand = "test";
            }
          );

          # The crate denies unwrap, expect and panic outside tests (see src/main.rs), so a panic-prone change fails here
          hoprdOperatorClippy = builders.local.callPackage nixLib.mkRustPackage (
            (mkBuildArgs { src = sources.main; })
            // {
              runClippy = true;
            }
          );

          stableToolchain = pkgs.pkgsBuildHost.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        in
        {
          packages =
            binaries
            // dockerImages
            // {
              hoprd-operator = builders.local.callPackage nixLib.mkRustPackage (mkBuildArgs { src = sources.main; });
              hoprd-operator-test = hoprdOperatorTest;
              hoprd-operator-clippy = hoprdOperatorClippy;
              default = self.packages.${system}.hoprd-operator;
            };

          checks = {
            inherit hoprdOperatorTest hoprdOperatorClippy;
          };

          apps = {
            audit = nixLib.mkAuditApp { };
          };

          devShells.default = nixLib.mkDevShell {
            rustToolchain = stableToolchain;
            shellName = "hoprd-operator";
            treefmtWrapper = config.treefmt.build.wrapper;
            extraPackages = with pkgs; [
              kubernetes-helm
              just
            ];
          };
        };

      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
        "x86_64-darwin"
      ];
    };
}

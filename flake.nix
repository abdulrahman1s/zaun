{
  description = "EasyPlay: run Windows games through umu-run inside a per-game bubblewrap sandbox";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      rust-overlay,
    }:
    let
      lib = nixpkgs.lib;

      cargoToml = lib.importTOML ./Cargo.toml;
      version = cargoToml.workspace.package.version or "0.0.0";
      # build.rs embeds this in `easyplay --version`; 12 hex digits like `git rev-parse --short=12`.
      commit =
        if self ? rev then
          builtins.substring 0 12 self.rev
        else if self ? dirtyRev then
          builtins.substring 0 12 self.dirtyRev + "-dirty"
        else
          "unknown";

      # Only what cargo needs, so editing docs/ does not rebuild the package.
      src = lib.fileset.toSource {
        root = ./.;
        fileset = lib.fileset.unions [
          ./Cargo.toml
          ./Cargo.lock
          ./crates
          (lib.fileset.maybeMissing ./assets)
        ];
      };

      # Builds the package against any nixpkgs instance; shared by packages and the overlay.
      mkEasyplay =
        pkgs:
        pkgs.callPackage ./nix/package.nix {
          # Single source of truth for the toolchain: rust-toolchain.toml.
          rustToolchain = (rust-overlay.lib.mkRustBin { } pkgs).fromRustupToolchainFile ./rust-toolchain.toml;
          inherit src version commit;
        };
    in
    {
      overlays.default = final: _prev: { easyplay = mkEasyplay final; };

      nixosModules = {
        easyplay = import ./nix/nixos-module.nix self;
        default = self.nixosModules.easyplay;
      };

      homeManagerModules = {
        easyplay = import ./nix/home-manager-module.nix self;
        default = self.homeManagerModules.easyplay;
      };
    }
    # x86_64 only (RELEASE_STRATEGY §2): nixpkgs' umu-launcher is an FHS environment with a
    # 32-bit (i686) half and does not evaluate on aarch64-linux.
    // flake-utils.lib.eachSystem [ "x86_64-linux" ] (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        rustBin = rust-overlay.lib.mkRustBin { } pkgs;
        toolchain = rustBin.fromRustupToolchainFile ./rust-toolchain.toml;
        easyplay = mkEasyplay pkgs;
      in
      {
        packages = {
          inherit easyplay;
          inherit (easyplay) easyplay-init;
          default = easyplay;
        };

        apps.default = {
          type = "app";
          program = lib.getExe easyplay;
          inherit (easyplay) meta;
        };

        checks = {
          inherit easyplay;
          nixos-module = import ./nix/nixos-test.nix { inherit self pkgs; };
        };

        devShells.default = pkgs.mkShell {
          packages = [
            toolchain
          ]
          ++ easyplay.hostTools
          ++ (with pkgs; [
            pkg-config
            sqlite
            cargo-nextest
            cargo-deny
            cargo-insta
            just
            mold
          ]);
          buildInputs = easyplay.runtimeLibs;
          LD_LIBRARY_PATH = lib.makeLibraryPath easyplay.runtimeLibs;
          RUST_BACKTRACE = "1";
          shellHook = ''
            echo "easyplay dev shell: $(rustc --version)"
          '';
        };

        formatter = pkgs.nixfmt; # nixfmt-rfc-style is now an alias of this
      }
    );
}

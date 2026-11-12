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
    flake-utils.lib.eachSystem [ "x86_64-linux" "aarch64-linux" ] (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        lib = pkgs.lib;

        # Single source of truth for the toolchain: rust-toolchain.toml.
        toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

        # Libraries eframe/winit/wgpu dlopen at run time.
        runtimeLibs = with pkgs; [
          libGL
          libxkbcommon
          wayland
          vulkan-loader
          udev
          xorg.libX11
          xorg.libXcursor
          xorg.libXi
          xorg.libXrandr
        ];

        # Host tools EasyPlay executes.
        hostTools = with pkgs; [
          bubblewrap
          umu-launcher
          xdg-utils
        ];

        cargoToml = lib.importTOML ./Cargo.toml;
        version = cargoToml.workspace.package.version or "0.0.0";

        rustPlatform = pkgs.makeRustPlatform {
          cargo = toolchain;
          rustc = toolchain;
        };

        easyplay = rustPlatform.buildRustPackage {
          pname = "easyplay";
          inherit version;
          src = lib.cleanSource ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [
            "-p"
            "easyplay"
            "-p"
            "easyplay-init"
          ];
          # Tests that need real bwrap/umu cannot run in the Nix build sandbox.
          doCheck = false;
          nativeBuildInputs = with pkgs; [
            pkg-config
            makeWrapper
          ];
          buildInputs = runtimeLibs;
          EASYPLAY_LIBEXEC_DIR = "${placeholder "out"}/libexec/easyplay";
          postInstall = ''
            mkdir -p $out/libexec/easyplay
            if [ -e $out/bin/easyplay-init ]; then
              mv $out/bin/easyplay-init $out/libexec/easyplay/
            fi
            if [ -d assets ]; then
              mkdir -p $out/share
              cp -r assets/share/. $out/share/ 2>/dev/null || true
            fi
            wrapProgram $out/bin/easyplay \
              --prefix PATH : ${lib.makeBinPath hostTools} \
              --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibs}
          '';
          meta = {
            description = "Run Windows games through umu-run inside a per-game bubblewrap sandbox";
            license = lib.licenses.gpl3Plus;
            mainProgram = "easyplay";
            platforms = lib.platforms.linux;
          };
        };
      in
      {
        packages = {
          inherit easyplay;
          default = easyplay;
        };

        apps.default = flake-utils.lib.mkApp { drv = easyplay; };

        devShells.default = pkgs.mkShell {
          packages =
            [ toolchain ]
            ++ hostTools
            ++ (with pkgs; [
              pkg-config
              sqlite
              cargo-nextest
              cargo-deny
              cargo-insta
              just
              mold
            ]);
          buildInputs = runtimeLibs;
          LD_LIBRARY_PATH = lib.makeLibraryPath runtimeLibs;
          RUST_BACKTRACE = "1";
          shellHook = ''
            echo "easyplay dev shell: $(rustc --version)"
          '';
        };

        formatter = pkgs.nixfmt-rfc-style;
      }
    );
}

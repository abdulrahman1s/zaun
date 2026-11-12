# EasyPlay and its static init shim, built from the workspace in this repository.
#
# Called from flake.nix and from overlays.default. `rustToolchain` is the toolchain pinned
# by rust-toolchain.toml (through rust-overlay); everything else comes from nixpkgs.
{
  lib,
  stdenv,
  makeRustPlatform,
  makeWrapper,
  installShellFiles,
  pkg-config,
  sqlite,
  writeText,
  bubblewrap,
  umu-launcher,
  xdg-utils,
  libGL,
  libxkbcommon,
  wayland,
  vulkan-loader,
  udev,
  libx11,
  libxcursor,
  libxi,
  libxrandr,
  rustToolchain,
  src,
  version,
  commit ? "unknown",
}:

let
  appId = "io.github.easyplay.EasyPlay";

  # Libraries eframe/winit/wgpu dlopen at run time.
  runtimeLibs = [
    libGL
    libxkbcommon
    wayland
    vulkan-loader
    udev
    libx11
    libxcursor
    libxi
    libxrandr
  ];

  # Host tools EasyPlay executes (RELEASE_STRATEGY §2: bubblewrap and umu-launcher on PATH).
  hostTools = [
    bubblewrap
    umu-launcher
    xdg-utils
  ];

  meta = {
    description = "Run Windows games through umu-run inside a per-game bubblewrap sandbox";
    homepage = "https://github.com/easyplay/easyplay";
    license = lib.licenses.gpl3Plus;
    platforms = lib.platforms.linux;
  };

  rustPlatform = makeRustPlatform {
    cargo = rustToolchain;
    rustc = rustToolchain;
  };
  cargoDeps = rustPlatform.importCargoLock { lockFile = "${src}/Cargo.lock"; };

  # The shim is bind-mounted into every sandbox, so it is linked statically against musl
  # (ARCHITECTURE §12) and depends on nothing from the store at run time. Rust's musl
  # targets link self-contained (bundled crt and libc.a), so the regular stdenv suffices;
  # only the shim is built for musl, while build scripts stay native.
  hostTarget = stdenv.hostPlatform.rust.rustcTarget;
  muslTarget = "${stdenv.hostPlatform.parsed.cpu.name}-unknown-linux-musl";
  muslToolchain = rustToolchain.override {
    targets = [
      hostTarget
      muslTarget
    ];
  };

  easyplay-init = stdenv.mkDerivation {
    pname = "easyplay-init";
    inherit version src cargoDeps;
    nativeBuildInputs = [
      rustPlatform.cargoSetupHook
      muslToolchain
    ];
    # The `init-release` profile is the fully specified shim profile (panic = abort, fat LTO,
    # stripped; Cargo.toml).
    buildPhase = ''
      runHook preBuild
      cargo build --offline --locked -j $NIX_BUILD_CORES \
        --profile init-release -p easyplay-init --target ${muslTarget}
      runHook postBuild
    '';
    installPhase = ''
      runHook preInstall
      install -Dm755 target/${muslTarget}/init-release/easyplay-init $out/bin/easyplay-init
      runHook postInstall
    '';
    # A static binary has nothing to patch, and its RPATH must stay empty.
    dontPatchELF = true;
    # The shim's tests fork and signal real processes; they run in `cargo nextest`.
    doCheck = false;
    meta = meta // {
      description = "EasyPlay's in-sandbox init shim";
      mainProgram = "easyplay-init";
    };
  };

  # Used only when the source tree does not ship its own desktop entry yet.
  # Content follows DESKTOP_INTEGRATION §2 exactly.
  fallbackDesktopEntry = writeText "${appId}.desktop" ''
    [Desktop Entry]
    Type=Application
    Version=1.5
    Name=EasyPlay
    GenericName=Windows Game Launcher
    Comment=Play Windows games with per-game protection
    Exec=easyplay open %F
    TryExec=easyplay
    Icon=${appId}
    Terminal=false
    Categories=Game;
    Keywords=windows;exe;wine;proton;games;launcher;sandbox;
    MimeType=application/vnd.microsoft.portable-executable;application/x-ms-dos-executable;application/x-msdos-program;application/x-msdownload;application/x-dosexec;application/x-msi;
    StartupNotify=true
    StartupWMClass=${appId}
    Actions=add-game;

    [Desktop Action add-game]
    Name=Add Game…
    Exec=easyplay --add
  '';
in
rustPlatform.buildRustPackage {
  pname = "easyplay";
  inherit version src cargoDeps;
  cargoBuildFlags = [
    "-p"
    "easyplay"
  ];
  # Tests that need real bwrap/umu cannot run in the Nix build sandbox.
  doCheck = false;

  nativeBuildInputs = [
    pkg-config
    makeWrapper
    installShellFiles
  ];
  # libsqlite3-sys currently resolves without its `bundled` feature and links the system
  # library through pkg-config; when the workspace bundles SQLite this is simply unused.
  buildInputs = runtimeLibs ++ [ sqlite ];

  # Compiled into easyplay-core's consts::LIBEXEC_DIR (RELEASE_STRATEGY §2.1).
  EASYPLAY_LIBEXEC_DIR = "${placeholder "out"}/libexec/easyplay";
  # There is no .git in the build sandbox; build.rs takes the commit from here.
  EASYPLAY_COMMIT = commit;

  postInstall = ''
    install -Dm755 ${easyplay-init}/bin/easyplay-init $out/libexec/easyplay/easyplay-init

    # Desktop entry, icons and AppStream metadata, when the source tree has them.
    if [ -d assets/share ]; then
      mkdir -p $out/share
      cp -r assets/share/. $out/share/
    fi
    if [ -d assets/desktop ]; then
      for f in assets/desktop/*.desktop; do
        [ -e "$f" ] && install -Dm644 "$f" -t $out/share/applications
      done
      for f in assets/desktop/*.metainfo.xml; do
        [ -e "$f" ] && install -Dm644 "$f" -t $out/share/metainfo
      done
    fi
    if [ -d assets/icons/hicolor ]; then
      mkdir -p $out/share/icons
      cp -r assets/icons/hicolor $out/share/icons/
    fi
    if [ ! -e $out/share/applications/${appId}.desktop ]; then
      install -Dm644 ${fallbackDesktopEntry} $out/share/applications/${appId}.desktop
    fi

    # Man pages and shell completions from the clap definitions (CLI-004).
    $out/bin/easyplay generate-assets --out generated
    installManPage generated/man/man1/*.1
    # bash-completion looks the file up by command name.
    installShellCompletion --bash --name easyplay generated/completions/easyplay.bash
    installShellCompletion --zsh generated/completions/_easyplay
    installShellCompletion --fish generated/completions/easyplay.fish

    wrapProgram $out/bin/easyplay \
      --prefix PATH : ${lib.makeBinPath hostTools} \
      --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibs}
  '';

  passthru = {
    inherit easyplay-init runtimeLibs hostTools;
  };

  meta = meta // {
    mainProgram = "easyplay";
  };
}

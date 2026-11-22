# Zaun and its static init shim, built from the workspace in this repository.
#
# Called from flake.nix and from overlays.default. `rustToolchain` is the toolchain pinned
# by rust-toolchain.toml (through rust-overlay); everything else comes from nixpkgs.
{
  lib,
  stdenv,
  makeRustPlatform,
  makeWrapper,
  patchelf,
  installShellFiles,
  removeReferencesTo,
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
  libopus,
  libpulseaudio,
  pipewire,
  ffmpeg_8-headless,
  libva,
  aws-lc,
  appimageTools,
  runCommand,
  simplex-chat-desktop,
  gmp,
  zlib,
  rustToolchain,
  src,
  version,
  commit ? "unknown",
  # A path outside the store keeps evaluation free of import-from-derivation
  # (`nix flake check --no-build` works); defaults to the lock file inside `src`.
  cargoLockFile ? "${src}/Cargo.lock",
}:

let
  appId = "io.github.zaun.Zaun";

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
    # Calls (ADR-0035): Opus, and sound in and out through PulseAudio (or PipeWire's).
    libopus
    libpulseaudio
    # Game streams (ADR-0036): a game window's pictures from the ScreenCast portal.
    pipewire
    # Game streams' hardware H.264 (ADR-0036): NVENC and VA-API through libavcodec (FFmpeg 8,
    # GPL build; the NVIDIA and VA-API drivers are loaded at run time).
    ffmpeg_8-headless
  ];

  # What the self-contained bundle (ADR-0041) loads from the system: its binaries link
  # libstdc++, libva (FFmpeg is built in), PipeWire, PulseAudio, Opus and udev, and load the
  # graphics libraries at run time. On NixOS, nix-ld provides them (`programs.zaun.selfUpdate`).
  bundleLibs = lib.filter (p: p != ffmpeg_8-headless) runtimeLibs ++ [
    stdenv.cc.cc.lib
    libva
    zlib
    gmp
  ];

  # Host tools Zaun executes (RELEASE_STRATEGY §2: bubblewrap and umu-launcher on PATH).
  hostTools = [
    bubblewrap
    umu-launcher
    xdg-utils
  ];

  meta = {
    description = "Run Windows games through umu-run inside a per-game bubblewrap sandbox";
    homepage = "https://github.com/abdulrahman1s/zaun";
    license = lib.licenses.gpl3Plus;
    platforms = lib.platforms.linux;
  };

  # The Chat engine (ADR-0033): the SimpleX chat core, `libsimplex.so` (AGPL-3.0), with the
  # GHC runtime and Haskell libraries it is linked against (`libHS*.so`) and the libffi and
  # libcrypto it was built with. nixpkgs has no library package for it, so it is taken from
  # the official desktop AppImage that nixpkgs already pins for simplex-chat-desktop (same
  # pname/version/src, so the extraction is the one nixpkgs itself builds). The AppImage is
  # published for x86_64-linux and aarch64-linux only; elsewhere this is null and the Chat
  # page says this build does not include it.
  #
  # The prebuilt objects are copied as they are (runCommand has no fixup phase: nothing is
  # stripped or patched). libsimplex lists every libHS*.so, libffi and libcrypto as DT_NEEDED
  # with `$ORIGIN` in its RUNPATH, so they resolve beside it; the only libraries it needs from
  # outside are libgmp.so.10 and libz.so.1, passed to the helper as `simplexLibraryPath`.
  simplexLib =
    let
      inherit (stdenv.hostPlatform) system;
      appimage = appimageTools.extract {
        inherit (simplex-chat-desktop) pname version;
        src = simplex-chat-desktop.passthru.sources.${system};
      };
    in
    if simplex-chat-desktop.passthru.sources ? ${system} then
      runCommand "zaun-simplex-lib-${simplex-chat-desktop.version}"
        {
          meta = {
            description = "SimpleX chat core library for Zaun's Chat helper";
            homepage = "https://github.com/simplex-chat/simplex-chat";
            license = lib.licenses.agpl3Only;
            sourceProvenance = [ lib.sourceTypes.binaryNativeCode ];
            platforms = simplex-chat-desktop.meta.platforms;
          };
        }
        ''
          # Not vlc/ (the desktop app's media player) and not libapp-lib.so (its JNI glue).
          res=${appimage}/usr/lib/app/resources
          mkdir -p $out/lib
          cp $res/libsimplex.so $res/libHS*.so $res/libffi.so.8 $res/libcrypto.so.3 $out/lib/
          chmod 0555 $out/lib/*
        ''
    else
      null;
  simplexLibraryPath = lib.makeLibraryPath [
    gmp
    zlib
  ];

  rustPlatform = makeRustPlatform {
    cargo = rustToolchain;
    rustc = rustToolchain;
  };
  cargoDeps = rustPlatform.importCargoLock { lockFile = cargoLockFile; };

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

  zaun-init = stdenv.mkDerivation {
    pname = "zaun-init";
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
        --profile init-release -p zaun-init --target ${muslTarget}
      runHook postBuild
    '';
    installPhase = ''
      runHook preInstall
      install -Dm755 target/${muslTarget}/init-release/zaun-init $out/bin/zaun-init
      runHook postInstall
    '';
    # A static binary has nothing to patch, and its RPATH must stay empty.
    dontPatchELF = true;
    # The shim's tests fork and signal real processes; they run in `cargo nextest`.
    doCheck = false;
    meta = meta // {
      description = "Zaun's in-sandbox init shim";
      mainProgram = "zaun-init";
    };
  };

  # Used only when the source tree does not ship its own desktop entry yet.
  # Content follows DESKTOP_INTEGRATION §2 exactly.
  fallbackDesktopEntry = writeText "${appId}.desktop" ''
    [Desktop Entry]
    Type=Application
    Version=1.5
    Name=Zaun
    GenericName=Windows Game Launcher
    Comment=Play Windows games with per-game protection
    Exec=zaun open %F
    TryExec=zaun
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
    Exec=zaun --add
  '';
in
rustPlatform.buildRustPackage {
  pname = "zaun";
  inherit version src cargoDeps;
  cargoBuildFlags = [
    "-p"
    "zaun"
    # The in-game overlay (ADR-0037), started by Zaun from libexec.
    "-p"
    "zaun-overlay"
  ]
  ++ lib.optionals (simplexLib != null) [
    "-p"
    "zaun-chat"
  ];
  # Tests that need real bwrap/umu cannot run in the Nix build sandbox.
  doCheck = false;

  nativeBuildInputs = [
    pkg-config
    makeWrapper
    installShellFiles
    removeReferencesTo
    patchelf
    # libspa/pipewire-sys generate their bindings with libclang (ADR-0036).
    rustPlatform.bindgenHook
  ];
  # libsqlite3-sys currently resolves without its `bundled` feature and links the system
  # library through pkg-config; when the workspace bundles SQLite this is simply unused.
  # Calls' DTLS (str0m → dimpl → aws-lc-rs, ADR-0035): nixpkgs' cargo hook sets
  # AWS_LC_SYS_USE_SYSTEM, so aws-lc-sys links this instead of building its own copy.
  buildInputs = runtimeLibs ++ [
    sqlite
    aws-lc
  ];

  # Compiled into zaun-core's consts::LIBEXEC_DIR (RELEASE_STRATEGY §2.1).
  ZAUN_LIBEXEC_DIR = "${placeholder "out"}/libexec/zaun";
  # There is no .git in the build sandbox; build.rs takes the commit from here.
  ZAUN_COMMIT = commit;

  postInstall = ''
    install -Dm755 ${zaun-init}/bin/zaun-init $out/libexec/zaun/zaun-init
    # The in-game overlay (ADR-0037): started by Zaun under bwrap, never by the user.
    install -Dm755 $out/bin/zaun-overlay $out/libexec/zaun/zaun-overlay
    rm $out/bin/zaun-overlay
    remove-references-to -t ${rustToolchain} $out/libexec/zaun/zaun-overlay
  ''
  # The Chat helper is AGPL-3.0 (it loads libsimplex into its process); Zaun talks to it
  # over pipes only. Its library folder holds links into simplexLib, so the helper's sandbox
  # binds a real directory (SimplexFactory binds the library's parent folder).
  + lib.optionalString (simplexLib != null) ''
    install -Dm755 $out/bin/zaun-chat $out/libexec/zaun/zaun-chat
    rm $out/bin/zaun-chat
    remove-references-to -t ${rustToolchain} $out/libexec/zaun/zaun-chat
    mkdir -p $out/libexec/zaun/simplex
    ln -s ${simplexLib}/lib/* $out/libexec/zaun/simplex/
  ''
  + ''

    # Desktop entry, icons and AppStream metadata, when the source tree has them.
    if [ -d assets/share ]; then
      mkdir -p $out/share
      cp -r assets/share/. $out/share/
    fi
    # The background service's user unit (ADR-0022) names the wrapped binary.
    if [ -f $out/share/systemd/user/zaun.service ]; then
      substituteInPlace $out/share/systemd/user/zaun.service \
        --replace-fail "ExecStart=/usr/bin/zaun " "ExecStart=$out/bin/zaun "
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

    # Licence texts, at the path the other packages use (DESKTOP_INTEGRATION §10).
    if [ -f LICENSE ]; then
      install -Dm644 LICENSE -t $out/share/licenses/zaun
    fi
    for f in assets/licenses/*.txt; do
      [ -e "$f" ] && install -Dm644 "$f" -t $out/share/licenses/zaun
    done

    # Man pages and shell completions from the clap definitions (CLI-004).
    $out/bin/zaun generate-assets --out generated
    installManPage generated/man/man1/*.1
    # bash-completion looks the file up by command name.
    installShellCompletion --bash --name zaun generated/completions/zaun.bash
    installShellCompletion --zsh generated/completions/_zaun
    installShellCompletion --fish generated/completions/zaun.fish

    # Panic locations in std embed the toolchain's store path, which would otherwise pull
    # the whole toolchain (about 1.2 GiB) into the runtime closure.
    remove-references-to -t ${rustToolchain} $out/bin/zaun

    # A session-wide `MANGOHUD=1` would load the overlay into Zaun's own window, where
    # its sampling thread runs as long as the service does. Games are unaffected: they get
    # their environment from the launch plan, never from this process.
    wrapProgram $out/bin/zaun \
      --set DISABLE_MANGOHUD 1 \
      --prefix PATH : ${lib.makeBinPath hostTools} \
      --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibs} \
      ${lib.optionalString (simplexLib != null) ''
        --set ZAUN_SIMPLEX_LIB $out/libexec/zaun/simplex/libsimplex.so \
        --set ZAUN_SIMPLEX_LIBRARY_PATH ${simplexLibraryPath}
      ''}
  '';

  # The wrapper's LD_LIBRARY_PATH reaches only the process it starts: the session supervisor
  # runs with a cleared environment and still has to dlopen libwayland-client (the Wayland
  # security context, ZN-SBX-001 otherwise). The run path is added after fixup, whose
  # shrink step would drop it again (no DT_NEEDED entry names these libraries).
  postFixup = ''
    patchelf --add-rpath ${lib.makeLibraryPath runtimeLibs} $out/bin/.zaun-wrapped
    # The overlay runs with a cleared environment and dlopens libwayland-client and the
    # Vulkan/GL loaders (ADR-0037).
    patchelf --add-rpath ${lib.makeLibraryPath runtimeLibs} $out/libexec/zaun/zaun-overlay
  '';

  disallowedReferences = [ rustToolchain ];

  passthru = {
    inherit
      zaun-init
      runtimeLibs
      bundleLibs
      hostTools
      simplexLib
      ;
  };

  meta = meta // {
    # libexec/zaun/zaun-chat is AGPL-3.0-only (ADR-0033); everything else GPL-3.0+.
    license = [ meta.license ] ++ lib.optional (simplexLib != null) lib.licenses.agpl3Only;
    mainProgram = "zaun";
  };
}

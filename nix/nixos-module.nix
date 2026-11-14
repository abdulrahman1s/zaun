# NixOS module for EasyPlay (exported as nixosModules.default and nixosModules.easyplay).
#
# Installs the package, offers it as a handler for Windows executables without changing any
# default application (DESKTOP_INTEGRATION §4), turns on 32-bit graphics drivers for games,
# checks that unprivileged user namespaces, which bubblewrap needs, are available, and can run
# EasyPlay's background service (ADR-0022).
self:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.programs.easyplay;
  inherit (lib)
    literalExpression
    mkEnableOption
    mkIf
    mkMerge
    mkOption
    types
    ;

  desktopId = "io.github.easyplay.EasyPlay.desktop";

  # The MimeType list of the desktop entry (DESKTOP_INTEGRATION §2).
  mimeTypes = [
    "application/vnd.microsoft.portable-executable"
    "application/x-ms-dos-executable"
    "application/x-msdos-program"
    "application/x-msdownload"
    "application/x-dosexec"
    "application/x-msi"
  ];

  sysctl = config.boot.kernel.sysctl;
  isOff = v: v == false || v == 0 || v == "0";
  usernsCloneSysctl = sysctl."kernel.unprivileged_userns_clone" or null;
  maxUserNamespaces = sysctl."user.max_user_namespaces" or null;

  # Kernels carrying the Debian/linux-hardened patch default `kernel.unprivileged_userns_clone`
  # to 0. nixpkgs no longer ships linux_hardened, but a custom kernel may still carry it.
  kernel = config.boot.kernelPackages.kernel;
  kernelLooksHardened = lib.hasInfix "hardened" (kernel.modDirVersion or kernel.version or "");
in
{
  options.programs.easyplay = {
    enable = mkEnableOption ''
      EasyPlay, which runs Windows games through umu-run inside a per-game bubblewrap
      sandbox. The package's wrapper already puts bubblewrap and umu-launcher on its PATH
    '';

    package = mkOption {
      type = types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.easyplay;
      defaultText = literalExpression "easyplay.packages.\${pkgs.stdenv.hostPlatform.system}.easyplay";
      description = ''
        The EasyPlay package to install. Defaults to the package built by this flake for the
        host platform. With `overlays.default` applied, `pkgs.easyplay` builds the same
        package against your own nixpkgs instead.
      '';
    };

    enable32BitGraphics = mkOption {
      type = types.bool;
      default = true;
      example = false;
      description = ''
        Whether to enable `hardware.graphics` together with its 32-bit driver set
        (`hardware.graphics.enable32Bit`, x86_64 only). Most Windows games, and parts of
        Proton itself, are 32-bit and fail to start without 32-bit OpenGL and Vulkan
        drivers. Both settings are applied with `mkDefault`, so an explicit value
        elsewhere in your configuration wins.
      '';
    };

    associateWindowsExecutables = mkOption {
      type = types.bool;
      default = true;
      example = false;
      description = ''
        Whether to list EasyPlay as an available handler for Windows executables (`.exe`,
        `.msi`) in the system-wide `mimeapps.list` (`xdg.mime.addedAssociations`). This only
        adds EasyPlay to "Open With" menus; it never sets a default application. Making
        EasyPlay the default is the user's choice, made in EasyPlay's onboarding or the file
        manager.
      '';
    };

    backgroundService.enable = mkEnableOption ''
      EasyPlay's background service (ADR-0022) for every user with a graphical session: the
      library stays loaded, so a Windows program opened from the file manager starts at
      once and EasyPlay's window opens without a cold start. It costs about 20 MiB while no
      window has been opened. Running games are not stopped when the service stops or
      restarts. After upgrading the package, `systemctl --user restart easyplay` runs the
      new version'';

    allowUnprivilegedUsernsClone = mkOption {
      type = types.bool;
      default = false;
      example = true;
      description = ''
        Whether to set the sysctl `kernel.unprivileged_userns_clone = 1`. bubblewrap needs
        unprivileged user namespaces. Stock NixOS kernels allow them and do not have this
        sysctl; only kernels carrying the linux-hardened or Debian patch do, and those
        default it to 0. Enable this only on such a kernel: it re-opens an attack surface the
        hardened kernel closes on purpose, so it is never set implicitly. On kernels without
        the sysctl, systemd-sysctl logs a harmless warning at boot.
      '';
    };
  };

  config = mkIf cfg.enable (mkMerge [
    {
      environment.systemPackages = [ cfg.package ];

      assertions = [
        {
          assertion = config.security.allowUserNamespaces;
          message = ''
            programs.easyplay: EasyPlay runs every game in a bubblewrap sandbox, which needs
            user namespaces, but security.allowUserNamespaces is false.
          '';
        }
        {
          # Reported once: security.allowUserNamespaces = false sets this sysctl to 0 itself.
          assertion =
            !config.security.allowUserNamespaces || maxUserNamespaces == null || !(isOff maxUserNamespaces);
          message = ''
            programs.easyplay: boot.kernel.sysctl."user.max_user_namespaces" is 0, which
            disables the user namespaces bubblewrap needs.
          '';
        }
        {
          assertion = cfg.allowUnprivilegedUsernsClone -> !(isOff usernsCloneSysctl);
          message = ''
            programs.easyplay: allowUnprivilegedUsernsClone is true but
            boot.kernel.sysctl."kernel.unprivileged_userns_clone" is set to 0 elsewhere.
          '';
        }
      ];

      warnings =
        lib.optional (!cfg.allowUnprivilegedUsernsClone && isOff usernsCloneSysctl) ''
          programs.easyplay: boot.kernel.sysctl."kernel.unprivileged_userns_clone" is 0, so
          bubblewrap cannot create user namespaces as a normal user and EasyPlay cannot start
          games. Remove that setting or set programs.easyplay.allowUnprivilegedUsernsClone.
        ''
        ++
          lib.optional (!cfg.allowUnprivilegedUsernsClone && usernsCloneSysctl == null && kernelLooksHardened)
            ''
              programs.easyplay: the kernel (${
                kernel.modDirVersion or kernel.version
              }) looks hardened; such kernels
              usually disable unprivileged user namespaces, which bubblewrap needs. If EasyPlay
              reports that sandboxes cannot be created, set
              programs.easyplay.allowUnprivilegedUsernsClone = true.
            '';
    }

    (mkIf cfg.backgroundService.enable {
      systemd.user.services.easyplay = {
        description = "EasyPlay in the background";
        documentation = [ "man:easyplay(1)" ];
        partOf = [ "graphical-session.target" ];
        after = [ "graphical-session.target" ];
        wantedBy = [ "graphical-session.target" ];
        # The tools a session-started EasyPlay finds (xdg-open, xdg-mime, gio, …), not
        # NixOS's minimal unit PATH; the package's wrapper adds bwrap and umu-run itself.
        environment.PATH = lib.mkForce "/run/wrappers/bin:/etc/profiles/per-user/%u/bin:/nix/var/nix/profiles/default/bin:/run/current-system/sw/bin";
        serviceConfig = {
          ExecStart = "${cfg.package}/bin/easyplay service";
          Restart = "on-failure";
          RestartSec = 5;
          # Games run in the service's cgroup; stopping the service must not end them.
          KillMode = "process";
        };
      };
    })

    (mkIf cfg.allowUnprivilegedUsernsClone {
      boot.kernel.sysctl."kernel.unprivileged_userns_clone" = 1;
    })

    (mkIf cfg.enable32BitGraphics {
      hardware.graphics = {
        enable = lib.mkDefault true;
        enable32Bit = lib.mkDefault pkgs.stdenv.hostPlatform.isx86_64;
      };
    })

    (mkIf cfg.associateWindowsExecutables {
      # Added associations only; xdg.mime.defaultApplications is deliberately left alone.
      xdg.mime.addedAssociations = lib.genAttrs mimeTypes (_: desktopId);
    })
  ]);
}

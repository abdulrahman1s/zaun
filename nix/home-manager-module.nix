# home-manager module for Zaun (exported as homeManagerModules.default and
# homeManagerModules.zaun).
#
# Installs the package for one user, can manage ~/.config/zaun/config.toml
# (CONFIGURATION §2) and can run Zaun's background service (ADR-0022). System-level requirements (32-bit graphics drivers, user namespaces)
# are not something home-manager can provide; on NixOS use the NixOS module for those.
self:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.programs.zaun;
  tomlFormat = pkgs.formats.toml { };
  inherit (lib)
    literalExpression
    mkEnableOption
    mkIf
    mkOption
    types
    ;
in
{
  options.programs.zaun = {
    enable = mkEnableOption ''
      Zaun, which runs Windows games through umu-run inside a per-game bubblewrap
      sandbox. The package's wrapper already puts bubblewrap and umu-launcher on its PATH
    '';

    package = mkOption {
      type = types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.zaun;
      defaultText = literalExpression "zaun.packages.\${pkgs.stdenv.hostPlatform.system}.zaun";
      description = ''
        The Zaun package to install. Defaults to the package built by this flake for the
        host platform.
      '';
    };

    backgroundService.enable = mkEnableOption ''
      Zaun's background service (ADR-0022): the library stays loaded, so a Windows
      program opened from the file manager starts at once and Zaun's window opens without
      a cold start. It costs about 20 MiB while no window has been opened. Running games are
      not stopped when the service stops or restarts'';

    settings = mkOption {
      inherit (tomlFormat) type;
      default = { };
      example = literalExpression ''
        {
          appearance.theme = "dark";
          protection.default_preset = "offline";
          runtime.auto_update_interval_hours = 48;
          launch.global_env = [ { name = "DXVK_HUD"; value = "fps"; } ];
        }
      '';
      description = ''
        Preferences written to {file}`$XDG_CONFIG_HOME/zaun/config.toml`. Keys and
        sections are those of CONFIGURATION.md §2 (`general`, `appearance`, `protection`,
        `runtime`, `launch`, `statistics`, `logging`, `integration`, `advanced`, `paths`,
        `onboarding`); `schema_version = 1` is added when absent. Keys left out take
        Zaun's defaults.

        When this is empty (the default) the file is not managed and Zaun owns it.
        When it is set, the file becomes a read-only link into the Nix store: changes made
        in Zaun's Settings screen replace the link for the current session and are
        overwritten by the next home-manager activation.
      '';
    };
  };

  config = mkIf cfg.enable {
    home.packages = [ cfg.package ];

    systemd.user.services.zaun = mkIf cfg.backgroundService.enable {
      Unit = {
        Description = "Zaun in the background";
        Documentation = [ "man:zaun(1)" ];
        PartOf = [ "graphical-session.target" ];
        After = [ "graphical-session.target" ];
      };
      Service = {
        ExecStart = "${cfg.package}/bin/zaun service";
        Restart = "on-failure";
        RestartSec = 5;
        # Games run in the service's cgroup; stopping the service must not end them.
        KillMode = "process";
      };
      Install.WantedBy = [ "graphical-session.target" ];
    };

    xdg.configFile."zaun/config.toml" = mkIf (cfg.settings != { }) {
      source = tomlFormat.generate "zaun-config.toml" ({ schema_version = 1; } // cfg.settings);
      # Zaun writes this file atomically (rename over the path); declarative settings win.
      force = true;
    };
  };
}

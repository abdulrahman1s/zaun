# home-manager module for EasyPlay (exported as homeManagerModules.default and
# homeManagerModules.easyplay).
#
# Installs the package for one user, can manage ~/.config/easyplay/config.toml
# (CONFIGURATION §2) and can run EasyPlay's background service (ADR-0022). System-level requirements (32-bit graphics drivers, user namespaces)
# are not something home-manager can provide; on NixOS use the NixOS module for those.
self:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.programs.easyplay;
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
        host platform.
      '';
    };

    backgroundService.enable = mkEnableOption ''
      EasyPlay's background service (ADR-0022): the library stays loaded, so a Windows
      program opened from the file manager starts at once and EasyPlay's window opens without
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
        Preferences written to {file}`$XDG_CONFIG_HOME/easyplay/config.toml`. Keys and
        sections are those of CONFIGURATION.md §2 (`general`, `appearance`, `protection`,
        `runtime`, `launch`, `statistics`, `logging`, `integration`, `advanced`, `paths`,
        `onboarding`); `schema_version = 1` is added when absent. Keys left out take
        EasyPlay's defaults.

        When this is empty (the default) the file is not managed and EasyPlay owns it.
        When it is set, the file becomes a read-only link into the Nix store: changes made
        in EasyPlay's Settings screen replace the link for the current session and are
        overwritten by the next home-manager activation.
      '';
    };
  };

  config = mkIf cfg.enable {
    home.packages = [ cfg.package ];

    systemd.user.services.easyplay = mkIf cfg.backgroundService.enable {
      Unit = {
        Description = "EasyPlay in the background";
        Documentation = [ "man:easyplay(1)" ];
        PartOf = [ "graphical-session.target" ];
        After = [ "graphical-session.target" ];
      };
      Service = {
        ExecStart = "${cfg.package}/bin/easyplay service";
        Restart = "on-failure";
        RestartSec = 5;
        # Games run in the service's cgroup; stopping the service must not end them.
        KillMode = "process";
      };
      Install.WantedBy = [ "graphical-session.target" ];
    };

    xdg.configFile."easyplay/config.toml" = mkIf (cfg.settings != { }) {
      source = tomlFormat.generate "easyplay-config.toml" ({ schema_version = 1; } // cfg.settings);
      # EasyPlay writes this file atomically (rename over the path); declarative settings win.
      force = true;
    };
  };
}

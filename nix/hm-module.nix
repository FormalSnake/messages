self:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.programs.messages;
  json = pkgs.formats.json { };
in
{
  options.programs.messages = {
    enable = lib.mkEnableOption "Messages, an iMessage client backed by BlueBubbles";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "messages.packages.\${system}.default";
      description = "The Messages package to install.";
    };

    autostart = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Start Messages with the graphical session through a systemd user service.";
    };

    environmentFile = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "/run/secrets/messages.env";
      description = ''
        File of `KEY=value` lines passed to the autostart service, for secrets
        that must stay out of the Nix store: `MESSAGES_SERVER_URL` with
        `MESSAGES_SERVER_PASSWORD`, `MESSAGES_AGENT_URL` with
        `MESSAGES_AGENT_TOKEN`, `MESSAGES_KLIPY_KEY`, `MESSAGES_CANARYLLM_KEY`.
        The app writes `~/.config/messages/config.json` itself, so that file
        is left unmanaged.
      '';
    };

    theme = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      example = {
        canvas = "#1c1917";
        text = "#b4bdc3";
        accent = "#6099c0";
      };
      description = ''
        Palette tokens written to `~/.config/messages/theme.json`. The names
        are the fields of `Palette` in `crates/desktop/src/theme.rs`. Leave
        empty to let another tool (matugen, for one) own the file.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];

    xdg.configFile."messages/theme.json" = lib.mkIf (cfg.theme != { }) {
      source = json.generate "messages-theme.json" cfg.theme;
    };

    systemd.user.services.messages = lib.mkIf cfg.autostart {
      Unit = {
        Description = "Messages";
        PartOf = [ "graphical-session.target" ];
        After = [ "graphical-session.target" ];
      };
      Service = {
        ExecStart = lib.getExe cfg.package;
        Restart = "on-failure";
      }
      // lib.optionalAttrs (cfg.environmentFile != null) { EnvironmentFile = cfg.environmentFile; };
      Install.WantedBy = [ "graphical-session.target" ];
    };
  };
}

self:
{
  config,
  lib,
  pkgs,
  utils,
  ...
}:
let
  cfg = config.services.lsendd;
in
{
  options.services.lsendd = {
    enable = lib.mkEnableOption "lsendd, the lsend daemon, as a systemd user service";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "lsend.packages.\${pkgs.stdenv.hostPlatform.system}.default";
      description = "The lsend package providing `lsendd` and `lsendctl`.";
    };

    openFirewall = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Whether to open port 53317 (TCP and UDP) for LocalSend.";
    };

    accept = lib.mkOption {
      type = lib.types.enum [
        "all"
        "known"
        "none"
      ];
      default = "known";
      description = "Which incoming requests are accepted automatically.";
    };

    onText = lib.mkOption {
      type = lib.types.str;
      default = "cat; echo";
      example = lib.literalExpression ''lib.getExe' pkgs.wl-clipboard "wl-copy"'';
      description = "Shell command run for each received text message, with the text on its stdin.";
    };

    systemd.target = lib.mkOption {
      type = lib.types.str;
      default = "default.target";
      example = "graphical-session.target";
      description = ''
        The systemd user target that starts lsendd. Use
        `graphical-session.target` if {option}`services.lsendd.onText` needs
        the graphical session, e.g. `wl-copy`.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];

    networking.firewall = lib.mkIf cfg.openFirewall {
      allowedTCPPorts = [ 53317 ];
      allowedUDPPorts = [ 53317 ];
    };

    systemd.user.services.lsendd = {
      description = "lsend daemon";
      wantedBy = [ cfg.systemd.target ];
      partOf = [ cfg.systemd.target ];
      after = [ cfg.systemd.target ];
      # `--on-text` is run through `sh -c`.
      path = [ pkgs.bash ];
      serviceConfig = {
        ExecStart = utils.escapeSystemdExecArgs [
          (lib.getExe' cfg.package "lsendd")
          "--accept"
          cfg.accept
          "--on-text"
          cfg.onText
        ];
        Restart = "on-failure";
      };
    };
  };
}

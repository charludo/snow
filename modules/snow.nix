{ lib, ... }:
with lib;
{
  options.snow = {
    tags = mkOption {
      type = types.listOf types.str;
      default = [ ];
      description = ''
        Tags associated with this machine. Used by `snow rebuild <tag>`;
      '';
    };

    useRemoteSudo = mkOption {
      type = types.bool;
      default = false;
      description = ''
        Whether to use remote sudo when rebuilding a remote machine
      '';
    };

    askSudoPassword = mkOption {
      type = types.nullOr types.bool;
      default = null;
      description = ''
        Whether to ask for the remote sudo password.
        If `useRemoteSudo` is set, this defaults to `true`
      '';
    };

    buildOnTarget = mkOption {
      type = types.bool;
      default = false;
      description = ''
        Whether to build directly on the target host
      '';
    };

    targetHost = mkOption {
      type = types.nullOr types.str;
      default = null;
      description = ''
        The SSH host for which the machine should be built by default.
        Takes precedence over `snow.buildOnTarget`
      '';
    };

    buildHost = mkOption {
      type = types.nullOr types.str;
      default = null;
      description = ''
        The SSH host on which the building should take place
      '';
    };

    vm.id = mkOption {
      type = types.nullOr types.int;
      default = null;
      description = "ID the VM should have";
    };

    vm.ip = mkOption {
      type = types.nullOr types.str;
      default = null;
      description = "IP address the VM should have";
    };

    vm.proxmoxHost = mkOption {
      type = types.nullOr types.str;
      default = null;
      description = "SSH proxmox host to use for this VM";
    };

    vm.proxmoxImageStore = mkOption {
      type = types.nullOr types.str;
      default = null;
      description = "Location where qcow VM images are stored for the given proxmox host";
    };

    vm.resizeDiskTo = mkOption {
      type = types.nullOr types.str;
      default = null;
      description = "Amount of GiB to which the disk size will be increased upon VM creation";
    };
  };
}

{ config, lib, ... }:
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

    useSubstitutes = mkOption {
      type = types.bool;
      default = false;
      description = ''
        Whether the target host should directly pull from substituters.
        Only useful if the connection between cache and target is faster than
        between target and build host.
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

    targetPort = mkOption {
      type = types.nullOr types.port;
      default = null;
      description = ''
        The SSH port to use when connecting to the target host.
        Can be overridden with `--target-port` / `-p` on the command line
      '';
    };

    buildHost = mkOption {
      type = types.nullOr types.str;
      default = null;
      description = ''
        The SSH host on which this machine wants to perform builds
      '';
    };

    buildMeOn = mkOption {
      type = types.nullOr types.str;
      default = null;
      description = ''
        The SSH host on which this machine wants to be rebuilt when some other
        host is rebuilding it via snow. `--build-host` passed to the CLI on the
        other host takes precedence
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

    vm.ha = mkOption {
      type = types.nullOr (
        types.submodule {
          options = {
            affinity.positive = mkOption {
              type = types.nullOr (types.nonEmptyListOf types.str);
              default = null;
              example = [
                "node1"
                "node2"
              ];
              description = ''
                Nodes of the positive HA node affinity rule the VM belongs in, in order of preference.
                Make sure that a corresponding affinity rule pre-exists.

                Mutually exclusive with `vm.ha.affinity.negative`.
              '';
            };

            affinity.negative = mkOption {
              type = types.nullOr (types.nonEmptyListOf types.str);
              default = null;
              example = [ "node1" ];
              description = ''
                Nodes of the negative HA node affinity rule the VM belongs in.
                Make sure that a corresponding affinity rule pre-exists.

                Mutually exclusive with `vm.ha.affinity.positive`.
              '';
            };

            strict = mkOption {
              type = types.bool;
              default = true;
              description = "Whether the HA node affinity rule is strict";
            };
          };
        }
      );
      default = null;
      description = ''
        HA settings of the VM. If set, the VM is HA-managed, and in a node affinity rule if one of `affinity.positive` and `affinity.negative` is set.
        If null, snow does not touch HA.
      '';
    };
  };

  config.assertions = [
    {
      assertion =
        config.snow.vm.ha == null
        || config.snow.vm.ha.affinity.positive == null
        || config.snow.vm.ha.affinity.negative == null;
      message = "snow.vm.ha.affinity: only one of `positive` and `negative` can be set, as a VM can only be in one node affinity rule";
    }
  ];
}

# Fake hosts for trying out `snow rebuild` with several hosts, without touching real machines.
# Their systems are trivial derivations, so only `-m build` makes sense. Built systems are cached,
# so bump `seed` to build them again.
#
#   alpha, beta  tags web, fake   build here, in a few seconds
#   alpha        is also a builder, with nix's defaults
#   gamma        tags db, fake    fails to build
#   delta        tags fake        wants to be built on a host that does not exist
{
  outputs =
    _:
    let
      seed = "20518";

      snow =
        extra:
        {
          tags = [ "fake" ];
          useRemoteSudo = false;
          askSudoPassword = null;
          useSubstitutes = false;
          targetHost = null;
          buildHost = null;
          buildMeOn = null;
          targetPort = null;
          builder = null;
          vm = null;
        }
        // extra;

      # The sandbox has no `sleep`, so work is a busy loop of `n` steps
      work = n: "i=0; while [ $i -lt ${toString n} ]; do i=$((i+1)); done";

      host = name: script: extra: {
        config = {
          snow = snow extra;
          system.build.toplevel = derivation {
            name = "nixos-system-${name}";
            system = "x86_64-linux";
            builder = "/bin/sh";
            args = [
              "-c"
              script
            ];
            inherit seed;
          };
        };
      };
    in
    {
      nixosConfigurations = {
        alpha =
          host "alpha"
            ''
              for step in 1 2 3; do echo "alpha step $step"; ${work 400000}; done
              echo > $out
            ''
            {
              tags = [
                "fake"
                "web"
              ];

              buildMeOn = "gsv";
              builder = {
                maxJobs = null;
                supportedFeatures = [ ];
              };
            };
        beta =
          host "beta"
            ''
              echo "beta compiling"; ${work 2000000}
              echo > $out
            ''
            {
              tags = [
                "fake"
                "web"
              ];
            };
        gamma =
          host "gamma"
            ''
              echo "gamma about to break"; ${work 300000}
              echo "error: gamma is broken" >&2
              exit 1
            ''
            {
              tags = [
                "fake"
                "db"
              ];
            };
        delta = host "delta" ''
          echo > $out
        '' { buildMeOn = "nonexistent.invalid"; };
      };
    };
}

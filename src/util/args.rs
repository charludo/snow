use crate::RebuildMode;
use clap::{Parser, Subcommand};

/// CLI wrapper for all commonly used nix, git and agenix commands, as well as a bunch of useful
/// helper scripts.
#[derive(Parser, Debug)]
#[command()]
#[command(version = env!("CARGO_PKG_VERSION"))]
pub(crate) struct Args {
    #[command(subcommand)]
    pub(crate) command: Commands,

    /// Enable [v]erbose debug logging, akin to --show-trace
    #[arg(long, short, global = true, display_order = 101)]
    pub(crate) verbose: bool,
}

#[derive(Subcommand, Debug)]
pub(crate) enum GitSubcommands {
    /// Pull new changes.
    Pull,

    /// Stage all files.
    Add,

    /// Stage and commit changes.
    Commit {
        /// Message with which to commit. If none given, changes will be amended to the previous
        /// commit.
        message: Option<String>,
    },

    /// Push to remote. Uses --force-with-lease.
    Push,

    /// Add, commit, push.
    All {
        /// Message with which to commit. If none given, changes will be amended to the previous
        /// commit.
        message: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum AgenixSubcommands {
    /// Update all secrets with a new set of masterkeys.
    UpdateMasterkeys,

    /// Edit the given secret.
    Edit { file: String },

    /// Rekey all secrets for the hosts requiring them.
    Rekey {
        /// Rekey secrets even if the applicable keys have not changed.
        #[arg(long, short, conflicts_with = "dummy", display_order = 1)]
        force: bool,

        /// Use a dummy key if no public key exists for a host.
        #[arg(long, short, conflicts_with = "force", display_order = 2)]
        dummy: bool,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum Commands {
    /// Rebuild the config for a given host, defaulting to the current host.
    Rebuild {
        nixos_configuration: Option<String>,

        /// Rebuild mode.
        #[arg(long, short, value_enum, default_value_t = RebuildMode::Switch, display_order = 1)]
        mode: RebuildMode,

        /// Target host. Attempts to read default value from nix config.
        #[arg(long, short, display_order = 2)]
        target_host: Option<String>,

        /// Build host. Attempts to read default value from nix config.
        #[arg(long, short, conflicts_with = "build_on_target", display_order = 3)]
        build_host: Option<String>,

        /// Build directly on the target instead of the local machine.
        #[arg(long, short = 'r', conflicts_with = "build_host", display_order = 4)]
        build_on_target: bool,

        /// Whether deployment requires sudo authentication on the target side.
        #[arg(long, short = 's', display_order = 5)]
        use_remote_sudo: bool,
    },

    /// Rebuild only the HomeManager config for the current user and host.
    Home { home_configuration: Option<String> },

    /// Create a new virtual machine and import it in Proxmox.
    Provision {
        vm_configuration: String,

        /// Whether to SSH into the newly created VM after setup is complete.
        #[arg(long, short)]
        login_after_setup: bool,

        /// Whether to rebuild the current host after setting the VM up, making its SSH handle
        /// available for use in the terminal.
        #[arg(long, short)]
        rebuild_host_machine: bool,
    },

    /// Enter the default shell specified in the current flake.nix, or the shell specified.
    Develop { shell_name: Option<String> },

    /// Enter a nix shell with the given packages installed.
    Shell { packages: Vec<String> },

    /// Evaluate the given nix expression.
    Eval {
        expression: String,

        /// Format output as JSON
        #[arg(long, short, conflicts_with = "raw")]
        json: bool,

        /// Force output to only contain un-escaped strings
        #[arg(long, short, conflicts_with = "json")]
        raw: bool,
    },

    /// Enter the nix repl, preloading the current flake including submodules.
    Repl,

    /// Secrets management.
    Agenix {
        #[command(subcommand)]
        subcommand: AgenixSubcommands,
    },

    /// Update a flake input. If none given, update all flake inputs.
    Update { input: Option<String> },

    /// Collect garbage for NixOS and HomeManager.
    Clean {
        /// Whether to perform a rebuild afterwards.
        #[arg(long, short)]
        rebuild: bool,
    },

    /// Interact with Git and Git submodules.
    Git {
        #[command(subcommand)]
        subcommand: GitSubcommands,
    },

    /// Run nix fmt.
    Fmt,
}

use clap::Parser;
use log::LevelFilter;
use std::sync::OnceLock;

mod util;
use util::*;
mod commands;
use commands::*;
mod options;
use options::*;
static LOG_LEVEL: OnceLock<LevelFilter> = OnceLock::new();

fn main() {
    let args = Args::parse();

    LOG_LEVEL.get_or_init(|| {
        if args.verbose {
            LevelFilter::Debug
        } else {
            LevelFilter::Info
        }
    });
    setup_logger(*LOG_LEVEL.get().unwrap());

    let result = match &args.command {
        Commands::Rebuild {
            nixos_configuration,
            mode,
            target_host,
            build_host,
            build_on_target,
            use_remote_sudo,
        } => rebuild(
            nixos_configuration,
            mode,
            target_host,
            build_host,
            *build_on_target,
            *use_remote_sudo,
        ),
        Commands::Home { home_configuration } => home(home_configuration),
        Commands::Provision {
            vm_configuration,
            login_after_setup,
            rebuild_host_machine,
        } => vm_new(vm_configuration, *login_after_setup, *rebuild_host_machine),
        Commands::Update { input } => update(input),
        Commands::Repl => repl(),
        Commands::Git { subcommand } => match subcommand {
            GitSubcommands::Add => git_add(),
            GitSubcommands::Commit { message } => git_commit(message),
            GitSubcommands::Pull => git_pull(),
            GitSubcommands::Push => git_push(),
            GitSubcommands::All { message } => git_all(message),
            GitSubcommands::Init => git_init(),
        },
        Commands::Agenix { subcommand } => match subcommand {
            AgenixSubcommands::UpdateMasterkeys => agenix_update_masterkeys(),
            AgenixSubcommands::Edit { file } => agenix_edit(file),
            AgenixSubcommands::Rekey { force, dummy } => agenix_rekey(*force, *dummy),
        },
        Commands::Clean { rebuild } => clean(*rebuild),
        Commands::Shell { packages } => shell(packages),
        Commands::Develop { shell_name } => develop(shell_name),
        Commands::Eval {
            expression,
            json,
            raw,
        } => eval(expression, *json, *raw),
        Commands::Fmt => fmt(),
    };

    if let Err(message) = result {
        log::error!("{}", message);
        std::process::exit(1);
    }
}

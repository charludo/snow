use clap::Parser;
use log::LevelFilter;
use std::path::Path;
use std::sync::OnceLock;

mod util;
use util::*;
mod commands;
use commands::*;
mod options;
use options::*;
static LOG_LEVEL: OnceLock<LevelFilter> = OnceLock::new();

fn main() {
    let mut argv = std::env::args_os();
    if argv
        .next()
        .is_some_and(|name| Path::new(&name).file_name() == Some(ASKPASS_NAME.as_ref()))
    {
        run_askpass(argv.collect());
    }
    let args = Args::parse();

    let level = match args.verbose {
        true => LevelFilter::Debug,
        false => LevelFilter::Info,
    };
    LOG_LEVEL.get_or_init(|| level);
    setup_logger(level);

    let result = match &args.command {
        Commands::Rebuild {
            selectors,
            exclude,
            mode,
            target_host,
            build_host,
            use_remote_sudo,
            ask_sudo_password,
            use_substitutes,
            target_port,
            builders,
            parallel,
            cache,
        } => rebuild(
            selectors,
            exclude,
            mode,
            &Flags {
                target_host: target_host.as_deref(),
                build_host: build_host.as_deref(),
                use_remote_sudo: *use_remote_sudo,
                ask_sudo_password: *ask_sudo_password,
                use_substitutes: *use_substitutes,
                target_port: *target_port,
                builders,
                parallel: *parallel,
                cache: cache.as_ref().map(Option::as_deref),
            },
        ),
        Commands::Home { home_configuration } => home(home_configuration),
        Commands::Provision {
            vm_configuration,
            login_after_setup,
            rebuild_host_machine,
        } => provision(vm_configuration, *login_after_setup, *rebuild_host_machine),
        Commands::Respec {
            selectors,
            exclude,
            reboot,
            yes,
        } => respec(selectors, exclude, *reboot, *yes),
        Commands::Hash { output } => build(output, true, None),
        Commands::Build { output, cache } => {
            build(output, false, cache.as_ref().map(Option::as_deref))
        }
        Commands::Run { output } => run(output),
        Commands::Cache { targets, to } => cache(targets, to),
        Commands::Update { input } => update(input),
        Commands::Repl => repl(),
        Commands::Why { derivation } => referrers_closure(derivation),
        Commands::Git {
            subcommand,
            submodules_only,
        } => match subcommand {
            GitSubcommands::Add => git_add(*submodules_only),
            GitSubcommands::Commit { message } => git_commit(message, *submodules_only),
            GitSubcommands::Pull => git_pull(*submodules_only),
            GitSubcommands::Push => git_push(*submodules_only),
            GitSubcommands::All { message } => git_all(message, *submodules_only),
            GitSubcommands::Init => git_init(*submodules_only),
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
        Commands::Tags { hostname } => tags(hostname),
        Commands::Config { hostname } => config(hostname),
        Commands::Fmt => fmt(),
        Commands::Check => check(),
        Commands::Bump { subcommand } => match subcommand {
            BumpSubcommands::Python { version } => bump_python(version),
        },
        Commands::Debug {
            nixos_configuration,
        } => debug_build(nixos_configuration),
        Commands::Assimilate {
            prepare,
            target,
            nixos_configuration,
        } => match (prepare, target, nixos_configuration) {
            (true, ..) => assimilate_prepare(),
            (false, Some(target), Some(nixos_configuration)) => {
                assimilate_run(target, nixos_configuration)
            }
            _ => Err(SnowError::Env(
                "assimilate needs a target and a nixosConfiguration".to_string(),
            )),
        },
    };
    askpass_cleanup();

    if let Err(message) = result {
        if !matches!(message, SnowError::Reported) {
            say(Tone::Bad, &message.to_string(), &[]);
        }
        std::process::exit(1);
    }
}

fn verbose() -> bool {
    LOG_LEVEL.get() == Some(&LevelFilter::Debug)
}

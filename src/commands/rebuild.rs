use crate::commands::util::{Tone, prefixed, read_from_repl, say, styled, wrap};
use crate::util::Result;
use crate::{SnowError, verbose};
use gethostname::gethostname;
use indicatif::FormattedDuration;
use inquire::Confirm;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::cell::LazyCell;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;
use users::get_current_username;

use super::runners::{Deploy, Job, JobResult, Live, SnowCommand};
use super::util::{BuilderConfig, SnowConfig};
use super::{RebuildMode, exist_untracked, git_add};

const REMOTE: &str = "→";
const LOCAL: &str = "●";

/// Settings from the command line, which take precedence over those of a host's snow config
#[derive(Default)]
pub(crate) struct Flags<'a> {
    pub(crate) target_host: Option<&'a str>,
    pub(crate) build_host: Option<&'a str>,
    pub(crate) use_remote_sudo: bool,
    pub(crate) ask_sudo_password: bool,
    pub(crate) use_substitutes: bool,
    pub(crate) target_port: Option<u16>,
    pub(crate) builders: &'a [String],
    pub(crate) parallel: Option<usize>,
}

/// How `nixos-rebuild` is run for one host
#[cfg_attr(test, derive(Clone))]
struct Plan {
    name: String,
    target_host: Option<String>,
    build_host: Option<String>,
    use_remote_sudo: bool,
    ask_sudo_password: bool,
    use_substitutes: bool,
    target_port: Option<u16>,
    builders: Option<String>,
    local: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Selection {
    Host(Box<SnowConfig>),
    Hosts(Selected),
}

#[derive(Deserialize)]
struct Selected {
    hosts: BTreeMap<String, SnowConfig>,
    broken: Vec<String>,
    unknown: Vec<String>,
}

#[derive(Deserialize)]
struct Toplevel {
    drv: String,
    out: String,
}

pub(crate) fn rebuild(
    selectors: &[String],
    exclude: &[String],
    mode: &RebuildMode,
    flags: &Flags,
) -> Result<()> {
    if exist_untracked()?
        && Confirm::new("Files exist which are untracked by git. If this rebuild depends on such a file, it will fail. Do you want to add them before proceeding?").with_default(true).prompt()?
    {
        git_add(false)?;
    }

    if !flags.builders.is_empty() {
        ensure_trusted()?;
    }

    let hostname = gethostname().into_string().unwrap_or_default();
    let started = Instant::now();
    let (selection, machines) = Live::run(|live| -> Result<_> {
        if !flags.builders.is_empty() {
            live.screen().set_status(format!(
                "Evaluating the builders: {}",
                builder_names(flags.builders).join(", ")
            ));
        }
        let machines = builders(flags.builders, &builder_configs(flags.builders)?)?;
        if selectors.is_empty() {
            live.screen()
                .set_status(format!("Evaluating the snow config of {hostname}"));
            let config = SnowConfig::get_snow_config(&hostname)?;
            return Ok((Selection::Host(Box::new(config)), machines));
        }
        let mut status = format!("Selecting hosts: {}", selectors.join(" "));
        if !exclude.is_empty() {
            status += &format!(", except {}", exclude.join(" "));
        }
        live.screen().set_status(status);
        Ok((select(selectors, exclude)?, machines))
    })?;

    let (selected, single) = match selection {
        Selection::Host(config) => {
            let name = selectors.first().unwrap_or(&hostname).clone();
            confirm_target(&name, &config, mode, flags, &hostname)?;
            let selected = Selected {
                hosts: BTreeMap::from([(name, *config)]),
                broken: Vec::new(),
                unknown: Vec::new(),
            };
            (selected, true)
        }
        Selection::Hosts(_) if flags.target_host.is_some() => {
            return Err(SnowError::Env(
                "--target-host can only be used with a single host".to_string(),
            ));
        }
        Selection::Hosts(selected) => (selected, false),
    };
    let (results, skipped) = Live::run(|live| {
        rebuild_many(
            live,
            selected,
            single,
            mode,
            flags,
            machines.as_deref(),
            &hostname,
        )
    })?;
    report(results, skipped, started)
}

fn confirm_target(
    name: &str,
    config: &SnowConfig,
    mode: &RebuildMode,
    flags: &Flags,
    hostname: &str,
) -> Result<()> {
    let target = flags.target_host.or(config.target_host.as_deref());
    let elsewhere = match target {
        None => name != hostname,
        Some(_) => target != config.target_host.as_deref(),
    };
    if mode.builds_only() || !elsewhere {
        return Ok(());
    }
    let confirmed = Confirm::new(&format!(
        "You are about to deploy the nixosConfiguration \"{name}\" to the target host \"{}\", overwriting the default target location \"{}\" for this host. Are you absolutely certain that this is what you meant to do?",
        target.unwrap_or(hostname),
        config.target_host.as_deref().unwrap_or("[not specified]"),
    ))
    .with_default(false)
    .with_help_message(&format!(
        "!!! This will erase the configuration currently deployed to {} !!!",
        target.map_or(format!("your local machine, \"{hostname}\""), str::to_string)
    ))
    .prompt()?;
    match confirmed {
        true => Ok(()),
        false => Err(SnowError::Reported),
    }
}

fn plan(
    name: &str,
    default_snow_config: &SnowConfig,
    hostname: &str,
    running_build_host: impl FnOnce() -> Option<String>,
    flags: &Flags,
    builders: Option<&str>,
) -> Result<Plan> {
    let rebuilding_self = name == hostname;

    // Resolve which build host to pass to nixos-rebuild.
    //
    // Priority (highest first):
    //   1. --builder CLI flags, with which it is built here and on those builders, and nowhere else
    //   2. --build-host CLI flag
    //   3. agreement between running host's buildHost and target's buildMeOn
    //   4. running host's buildHost alone
    //   5. target host's buildMeOn alone
    let build_host = if !flags.builders.is_empty() {
        None
    } else if let Some(cli_bh) = flags.build_host {
        (!cli_bh.is_empty()).then(|| cli_bh.to_string())
    } else {
        let running_build_host = if rebuilding_self {
            default_snow_config.build_host.clone()
        } else {
            running_build_host()
        };

        let target_build_me_on = if rebuilding_self {
            None
        } else {
            default_snow_config.build_me_on.clone()
        };

        match (running_build_host, target_build_me_on) {
            (Some(a), Some(b)) if a == b => Some(a),
            (Some(a), Some(b)) => {
                return Err(SnowError::SnowConfig(format!(
                    "build host conflict: this machine (\"{hostname}\") is configured to \
                     build on \"{a}\", but target \"{name}\" wants to be \
                     built on \"{b}\"; use --build-host/-b to select one"
                )));
            }
            (a, b) => a.or(b),
        }
    };

    let use_remote_sudo = flags.use_remote_sudo || default_snow_config.use_remote_sudo;
    Ok(Plan {
        name: name.to_string(),
        target_host: flags
            .target_host
            .map(str::to_string)
            .or(default_snow_config.target_host.clone()),
        build_host,
        use_remote_sudo,
        ask_sudo_password: flags.ask_sudo_password
            || default_snow_config
                .ask_sudo_password
                .unwrap_or(use_remote_sudo),
        use_substitutes: flags.use_substitutes || default_snow_config.use_substitutes,
        target_port: flags.target_port.or(default_snow_config.target_port),
        builders: builders.map(str::to_string),
        local: rebuilding_self,
    })
}

impl Plan {
    /// The job running `command`, showing `note` until it deploys
    fn job(&self, mode: &RebuildMode, store_path: Option<&str>, note: &str) -> Job {
        Job {
            name: self.name.clone(),
            command: self.command(mode, store_path),
            last: self.local && !mode.builds_only(),
            note: note.to_string(),
            note_queued: false,
            deploy: match (mode.builds_only(), store_path) {
                (true, _) => Deploy::Never,
                (false, Some(_)) => Deploy::Now,
                (false, None) => Deploy::Later(self.target_host.clone()),
            },
        }
    }

    fn command(&self, mode: &RebuildMode, store_path: Option<&str>) -> SnowCommand {
        let build_only = mode.builds_only();

        let mut args = vec![mode.to_string()];
        match store_path {
            Some(store_path) => args.extend(["--store-path".to_string(), store_path.to_string()]),
            None => args.extend(["--flake".to_string(), wrap(&self.name, true)]),
        }

        if let Some(target_host) = &self.target_host
            && !build_only
        {
            args.extend(["--target-host".to_string(), target_host.clone()]);
        }

        // An already built system is copied from here
        if store_path.is_none() {
            match (&self.build_host, &self.builders) {
                (Some(build_host), _) => {
                    args.extend(["--build-host".to_string(), build_host.to_string()])
                }
                (None, Some(builders)) => args.extend(builder_args(builders)),
                (None, None) => {}
            }
        }

        if self.use_remote_sudo {
            args.push("--sudo".to_string());
        }
        if self.ask_sudo_password {
            args.push("--ask-sudo-password".to_string());
        }
        if self.use_substitutes {
            args.push("--use-substitutes".to_string());
        }
        if verbose() && store_path.is_none() {
            args.push("--show-trace".to_string());
        }

        let mut command = SnowCommand::new_nix(
            "nixos-rebuild".to_string(),
            args.iter().map(String::as_str).collect(),
            self.local && !build_only,
        );
        if let Some(port) = self.target_port {
            let opts = match std::env::var("NIX_SSHOPTS") {
                Ok(opts) if !opts.is_empty() => format!("{opts} -p {port}"),
                _ => format!("-p {port}"),
            };
            command.set_env("NIX_SSHOPTS", &opts);
        }
        command
    }
}

fn plural(count: usize, noun: &str) -> String {
    match count {
        1 => format!("1 {noun}"),
        _ => format!("{count} {noun}s"),
    }
}

/// How many hosts are rebuilt on each build host, e.g. "1 host on gsv and 2 on abc"
fn on_build_hosts(counts: &[(&str, usize)], noun: Option<&str>) -> String {
    let parts = counts
        .iter()
        .enumerate()
        .map(|(index, (host, count))| match (index, noun) {
            (0, Some(noun)) => format!("{} on {host}", plural(*count, noun)),
            _ => format!("{count} on {host}"),
        });
    listing(&Vec::from_iter(parts))
}

/// `items` in a sentence, e.g. "gsv", "gsv and abc" or "gsv, abc and here"
fn listing(items: &[impl AsRef<str>]) -> String {
    match items {
        [] => String::new(),
        [item] => item.as_ref().to_string(),
        [rest @ .., last] => {
            let rest = Vec::from_iter(rest.iter().map(AsRef::as_ref));
            format!("{} and {}", rest.join(", "), last.as_ref())
        }
    }
}

/// The `snow.builder` of each of `builders` given as nixosConfiguration
fn builder_configs(builders: &[String]) -> Result<BTreeMap<String, Option<BuilderConfig>>> {
    let names = Vec::from_iter(
        builders
            .iter()
            .filter(|builder| !builder.contains("://"))
            .map(|builder| nix_string(builder_name(builder))),
    );
    if names.is_empty() {
        return Ok(BTreeMap::new());
    }
    let expr = format!(
        "cs: builtins.listToAttrs (map (name: {{ inherit name; value = cs.${{name}}.config.snow.builder; }}) \
         (builtins.filter (name: cs ? ${{name}}) [ {} ]))",
        names.join(" ")
    );
    apply(&expr)
        .map_err(|e| SnowError::Nix(format!("could not evaluate the builders: {}", e.message())))
}

/// `builders` as the nix `builders` setting
fn builders(
    builders: &[String],
    configs: &BTreeMap<String, Option<BuilderConfig>>,
) -> Result<Option<String>> {
    if builders.is_empty() {
        return Ok(None);
    }
    let mut machines = Vec::new();
    for builder in builders {
        if builder.contains("://") {
            machines.push(builder.clone());
            continue;
        }
        let name = builder_name(builder);
        let (jobs, features) = match configs.get(name) {
            Some(Some(config)) => (config.max_jobs, config.supported_features.join(",")),
            Some(None) => {
                return Err(SnowError::SnowConfig(format!(
                    "{name} has no `snow.builder` configuration. Set it, or pass a full nix machine spec"
                )));
            }
            None => (None, String::new()),
        };
        let jobs = jobs.map_or("-".to_string(), |jobs| jobs.to_string());
        let features = match features.is_empty() {
            true => "-".to_string(),
            false => features,
        };
        machines.push(format!("ssh-ng://{builder} - - {jobs} 1 {features}"));
    }
    Ok(Some(machines.join(" ; ")))
}

fn builder_name(builder: &str) -> &str {
    let uri = builder.split_whitespace().next().unwrap_or(builder);
    let host = uri.split_once("://").map_or(uri, |(_, rest)| rest);
    let host = host.split('?').next().unwrap_or(host);
    host.rsplit_once('@').map_or(host, |(_, host)| host)
}

fn builder_names(builders: &[String]) -> Vec<&str> {
    Vec::from_iter(builders.iter().map(|builder| builder_name(builder)))
}

fn builder_args(builders: &str) -> Vec<String> {
    [
        "--builders",
        builders,
        "--option",
        "builders-use-substitutes",
        "true",
    ]
    .map(str::to_string)
    .to_vec()
}

/// Fail unless the nix daemon trusts this user, without which it ignores `builders`
fn ensure_trusted() -> Result<()> {
    let info = SnowCommand::new_nix("nix".to_string(), vec!["store", "info", "--json"], false)
        .run_with_return()?;
    let info: serde_json::Value = serde_json::from_str(&info)?;
    // Older versions of nix report 1 or 0
    if info["trusted"] == true || info["trusted"] == 1 {
        return Ok(());
    }
    Err(SnowError::Env(
        "--builder requires this user to be trusted by the nix daemon (`nix.settings.trusted-users`), \
         as otherwise it ignores the builders"
            .to_string(),
    ))
}

/// The JSON of `expr` applied to the `nixosConfigurations` of the flake
fn apply<T: DeserializeOwned>(expr: &str) -> Result<T> {
    let json = read_from_repl("nixosConfigurations", vec!["--json", "--apply", expr])?;
    Ok(serde_json::from_str(&json)?)
}

fn nix_string(s: &str) -> String {
    serde_json::Value::from(s).to_string().replace("${", "\\${")
}

/// Select hosts in one evaluation: those matching any of `selectors`, but none of `exclude`, where
/// `a+b` matches hosts that match both `a` and `b`, each a host, a tag, or `*` for all.
fn select(selectors: &[String], exclude: &[String]) -> Result<Selection> {
    let single = match (selectors, exclude) {
        ([name], []) if name != "*" && !name.contains('+') => Some(name),
        _ => None,
    };
    fn atoms(selectors: &[String]) -> Vec<Vec<&str>> {
        selectors.iter().map(|s| s.split('+').collect()).collect()
    }
    let args = serde_json::json!({
        "single": single,
        "include": atoms(selectors),
        "exclude": atoms(exclude),
    });
    let expr = format!(
        r#"cs: let
          inherit (builtins.fromJSON {args}) single include exclude;
          names = builtins.attrNames cs;
          results = map (host: {{
            inherit host;
            snow = let snow = cs.${{host}}.config.snow; in builtins.tryEval (builtins.deepSeq snow snow);
          }}) names;
          ok = builtins.filter (r: r.snow.success) results;
          has = r: atom: atom == "*" || atom == r.host || builtins.elem atom r.snow.value.tags;
          matches = r: builtins.any (builtins.all (has r));
          known = atom: atom == "*" || builtins.elem atom names || builtins.any (r: builtins.elem atom r.snow.value.tags) ok;
        in
        if single != null && builtins.hasAttr single cs then {{ host = cs.${{single}}.config.snow; }}
        else {{ hosts = {{
          hosts = builtins.listToAttrs (map (r: {{ name = r.host; value = r.snow.value; }})
            (builtins.filter (r: matches r include && !(matches r exclude)) ok));
          broken = map (r: r.host) (builtins.filter (r: !r.snow.success) results);
          unknown = builtins.filter (atom: !(known atom)) (builtins.concatLists (include ++ exclude));
        }}; }}"#,
        args = nix_string(&args.to_string()),
    );
    apply(&expr).map_err(|e| {
        SnowError::Nix(match single {
            None => format!("could not select hosts: {}", e.message()),
            Some(name) => format!(
                "could not read snow config for host {name}: {}",
                e.message()
            ),
        })
    })
}

fn rebuild_many(
    live: &Live,
    Selected {
        hosts: configs,
        broken,
        unknown,
    }: Selected,
    single: bool,
    mode: &RebuildMode,
    flags: &Flags,
    machines: Option<&str>,
    hostname: &str,
) -> Result<(Vec<JobResult>, usize)> {
    if !unknown.is_empty() {
        // Most likely, the shell expanded an unquoted `*` into the files here
        let hint = match unknown.iter().any(|atom| Path::new(atom).exists()) {
            true => " (quote '*', so that the shell does not expand it)",
            false => "",
        };
        return Err(SnowError::SnowConfig(format!(
            "there are no hosts or tags named {}{hint}",
            unknown.join(", ")
        )));
    }

    let mut messages = Vec::new();
    if !broken.is_empty() {
        messages.push((
            Tone::Warn,
            None,
            format!(
                "Skipping hosts that could not be evaluated: {}",
                broken.join(", ")
            ),
        ));
    }
    if configs.is_empty() {
        return Err(SnowError::SnowConfig("no hosts are selected".to_string()));
    }
    let build_only = mode.builds_only();

    let running = LazyCell::new(|| match configs.get(hostname) {
        Some(config) => (config.build_host.clone(), Some(config.parallel)),
        None => SnowConfig::get_snow_config(hostname)
            .map_or((None, None), |c| (c.build_host, Some(c.parallel))),
    });
    let mut results = Vec::new();
    let mut plans = Vec::new();
    let mut untargeted = Vec::new();
    for (name, config) in &configs {
        let plan = plan(
            name,
            config,
            hostname,
            || running.0.clone(),
            flags,
            machines,
        );
        match plan {
            // Without a target, it would be deployed to the running host
            Ok(plan) if plan.target_host.is_none() && !plan.local && !build_only && !single => {
                untargeted.push(name.as_str());
            }
            Ok(plan) => plans.push(plan),
            Err(e) => {
                messages.push((Tone::Bad, Some(name.clone()), e.message()));
                results.push(JobResult::new(name, Err(e)));
            }
        }
    }
    if !untargeted.is_empty() {
        messages.push((
            Tone::Warn,
            None,
            format!(
                "Skipping hosts without a targetHost: {}",
                untargeted.join(", ")
            ),
        ));
    }
    if plans.is_empty() && results.is_empty() {
        return Err(SnowError::SnowConfig(format!(
            "none of the {} selected hosts has a targetHost",
            untargeted.len()
        )));
    }
    let skipped = broken.len() + untargeted.len();
    let names = Vec::from_iter(
        configs
            .keys()
            .map(String::as_str)
            .filter(|name| !untargeted.contains(name)),
    );
    let mut screen = live.screen();
    let message = format!("nixos-rebuild {mode}");
    let details = [plural(names.len(), "host"), names.join(", ")];
    screen.emit(move || {
        say(
            Tone::Info,
            &message,
            &details.each_ref().map(String::as_str),
        )
    });
    for (tone, prefix, text) in messages {
        screen.message(tone, prefix.as_deref(), &text);
    }
    drop(screen);

    if plans.is_empty() {
        live.screen()
            .set_retry(retry_command(&failures(&results), mode, flags));
        return Ok((results, skipped));
    }

    let mut remote: BTreeMap<&str, Vec<&Plan>> = BTreeMap::new();
    let mut here = Vec::new();
    for plan in &plans {
        match &plan.build_host {
            Some(build_host) => remote.entry(build_host).or_default().push(plan),
            None => here.push(plan),
        }
    }
    let remote_hosts: usize = remote.values().map(Vec::len).sum();
    let per_build_host = Vec::from_iter(remote.iter().map(|(host, plans)| (*host, plans.len())));
    let verb = if build_only { "Building" } else { "Deploying" };
    let parallel = match (flags.parallel, plans.len()) {
        (Some(parallel), _) => parallel,
        (None, 1) => 1,
        (None, _) => running.1.ok_or_else(|| {
            SnowError::SnowConfig(format!(
                "could not evaluate snow.parallel of this machine (\"{hostname}\"), \
                 pass --parallel to specify how many hosts to rebuild at once"
            ))
        })?,
    };
    let (built_only, job_results) = live.run_jobs(parallel, |queue| {
        for (build_host, plans) in &remote {
            live.screen()
                .add_group(&format!("{REMOTE} {build_host}"), true);
            queue.reserve(&Vec::from_iter(plans.iter().map(|plan| plan.name.as_str())));
            queue.send(Vec::from_iter(
                plans.iter().map(|plan| plan.job(mode, None, "building")),
            ));
        }
        if here.is_empty() {
            live.screen().set_status(format!(
                "{verb} {}",
                on_build_hosts(&per_build_host, Some("host"))
            ));
            return Vec::new();
        }
        let names = Vec::from_iter(here.iter().map(|plan| plan.name.as_str()));
        let mut group = format!("{LOCAL} {hostname}");
        for builder in builder_names(flags.builders) {
            group += &format!(" + {builder}");
        }
        let build = live.screen().add_group(&group, false);
        queue.reserve(&names);
        let built = build_here(
            live,
            build,
            hostname,
            names,
            (remote_hosts > 0).then(|| on_build_hosts(&per_build_host, None)),
            flags.builders,
            machines,
        );
        let mut built_only = Vec::new();
        let mut to_send = Vec::new();
        for plan in &here {
            // Hosts not built here are rebuilt from the flake, which finds their own error
            match built.as_ref().map(|built| built.get(&plan.name)) {
                Some(Some(_)) if build_only => {
                    queue.complete(&plan.name, "built");
                    built_only.push(JobResult::new(&plan.name, Ok(())));
                }
                Some(Some(store_path)) => {
                    to_send.push(plan.job(mode, Some(store_path), "deploying"))
                }
                Some(None) => {
                    let mut job = plan.job(mode, None, "failed to build, determining cause");
                    job.note_queued = true;
                    to_send.push(job);
                }
                None => to_send.push(plan.job(mode, None, "building on its own")),
            }
        }
        if !to_send.is_empty() {
            live.screen()
                .set_status(format!("{verb} {}", plural(to_send.len(), "host")));
        }
        queue.send(to_send);
        built_only
    });
    results.extend(built_only);
    results.extend(job_results);
    let failed = failures(&results);
    if !failed.is_empty() {
        live.screen().set_retry(retry_command(&failed, mode, flags));
    }
    Ok((results, skipped))
}

fn failures<'a>(results: impl IntoIterator<Item = &'a JobResult>) -> Vec<&'a str> {
    let mut failures = Vec::from_iter(
        results
            .into_iter()
            .filter(|result| result.result.is_err())
            .map(|result| result.name.as_str()),
    );
    failures.sort();
    failures
}

fn report(mut results: Vec<JobResult>, skipped: usize, started: Instant) -> Result<()> {
    results.sort_by(|a, b| a.name.cmp(&b.name));
    let failures = failures(&results);
    if !styled() && !verbose() {
        for result in results.iter().filter(|result| result.result.is_err()) {
            for line in &result.output {
                eprintln!("{}", prefixed(&result.name, line));
            }
        }
    }

    let mut details = Vec::new();
    if !failures.is_empty() {
        details.push(format!("{} failed", failures.len()));
    }
    if skipped > 0 {
        details.push(format!("{skipped} skipped"));
    }
    details.push(FormattedDuration(started.elapsed()).to_string());
    say(
        match failures.is_empty() {
            true => Tone::Good,
            false => Tone::Bad,
        },
        &format!(
            "Rebuilt {} of {}",
            results.len() - failures.len(),
            plural(results.len(), "host")
        ),
        &Vec::from_iter(details.iter().map(String::as_str)),
    );
    match failures.is_empty() {
        true => Ok(()),
        false => Err(SnowError::Reported),
    }
}

fn retry_command(hosts: &[&str], mode: &RebuildMode, flags: &Flags) -> Vec<String> {
    let quote = |arg: &str| match !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "@%_+=:,./-".contains(c))
    {
        true => arg.to_string(),
        false => format!("'{}'", arg.replace('\'', r"'\''")),
    };
    let mut args = vec!["snow".to_string()];
    if verbose() {
        args.push("-v".to_string());
    }
    args.push("rebuild".to_string());
    args.extend(hosts.iter().map(|host| quote(host)));
    args.extend(["-m".to_string(), mode.to_string()]);
    if let Some(target_host) = flags.target_host {
        args.extend(["-t".to_string(), quote(target_host)]);
    }
    if let Some(build_host) = flags.build_host {
        args.extend(["-b".to_string(), quote(build_host)]);
    }
    if flags.ask_sudo_password {
        args.push("-a".to_string());
    } else if flags.use_remote_sudo {
        args.push("-s".to_string());
    }
    if flags.use_substitutes {
        args.push("-c".to_string());
    }
    if let Some(port) = flags.target_port {
        args.extend(["-p".to_string(), port.to_string()]);
    }
    for builder in flags.builders {
        args.extend(["-B".to_string(), quote(builder)]);
    }
    if let Some(parallel) = flags.parallel {
        args.extend(["-P".to_string(), parallel.to_string()]);
    }
    args
}

fn build_here(
    live: &Live,
    row: usize,
    hostname: &str,
    names: Vec<&str>,
    remote: Option<String>,
    builders: &[String],
    machines: Option<&str>,
) -> Option<BTreeMap<String, String>> {
    let count = names.len();
    live.screen()
        .set_status(format!("Evaluating {}", plural(count, "system")));
    let expr = format!(
        "cs: builtins.listToAttrs (map (name: {{ inherit name; value = let t = \
         cs.${{name}}.config.system.build.toplevel; in {{ drv = t.drvPath; out = t.outPath; }}; }}) \
         [ {} ])",
        Vec::from_iter(names.iter().map(|name| nix_string(name))).join(" ")
    );
    let toplevels: BTreeMap<String, Toplevel> = match apply(&expr) {
        Ok(toplevels) => toplevels,
        Err(e) => {
            let warning = format!(
                "Could not evaluate the hosts together, so each is rebuilt on its own: {}",
                e.message()
            );
            live.screen().message(Tone::Warn, None, &warning);
            live.screen()
                .complete_row(row, false, "could not evaluate them together");
            live.screen().set_status(format!(
                "Evaluation failed, trying {} separately",
                plural(count, "host")
            ));
            return None;
        }
    };

    // Derivations, so they are not evaluated again, and as many built as possible
    let installables: Vec<String> = toplevels
        .values()
        .map(|toplevel| format!("{}^*", toplevel.drv))
        .collect();
    let builder_args = machines.map(builder_args).unwrap_or_default();
    let mut args = vec!["build", "--keep-going", "--no-link"];
    args.extend(builder_args.iter().map(String::as_str));
    args.extend(installables.iter().map(String::as_str));
    let place = match builders.is_empty() {
        true => "here".to_string(),
        false => format!("here and on {}", listing(&builder_names(builders))),
    };
    let mut status = format!("Building {} {place}", plural(count, "system"));
    if let Some(remote) = remote {
        status += &format!(", {remote}");
    }
    live.screen().set_status(status);
    let result = live.run_step(
        row,
        Job {
            name: hostname.to_string(),
            command: SnowCommand::new_nix("nix".to_string(), args, false),
            last: false,
            note: String::new(),
            note_queued: false,
            deploy: Deploy::Never,
        },
    );

    let (built, failed): (BTreeMap<_, _>, BTreeMap<_, _>) = toplevels
        .into_iter()
        .map(|(name, toplevel)| (name, toplevel.out))
        .partition(|(_, out)| Path::new(out).exists());
    if result.result.is_err() && !failed.is_empty() {
        live.screen().set_status(format!(
            "Determining why {} failed to build",
            plural(failed.len(), "host")
        ));
    }
    Some(built)
}

pub(crate) fn home(home_configuration: &Option<String>) -> Result<()> {
    let username_os = get_current_username().unwrap_or_default();
    let username = username_os.to_str().unwrap_or_default();
    let hostname_os = gethostname();
    let hostname = hostname_os.to_str().unwrap_or_default();
    let default_target = if !username.is_empty() && !hostname.is_empty() {
        Some(format!("{username}@{hostname}"))
    } else {
        None
    };
    let target = match (&home_configuration, &default_target) {
        (Some(target), _) => target.as_str(),
        (None, Some(default_target)) => default_target,
        _ => {
            return Err(SnowError::Env(
                "failed to read username/hostname".to_string(),
            ));
        }
    };

    let mut command = SnowCommand::new_nix(
        "home-manager".to_string(),
        vec!["switch", "--flake", wrap(target, true).as_str()],
        false,
    );
    if verbose() {
        command.append_arg("--show-trace");
    }
    command.run_progress(target.to_string())
}

#[test]
fn test_plan_command() {
    let plan = Plan {
        name: "web".to_string(),
        target_host: Some("paki@web".to_string()),
        build_host: Some("gsv".to_string()),
        use_remote_sudo: true,
        ask_sudo_password: false,
        use_substitutes: true,
        target_port: Some(2222),
        builders: Some("ssh-ng://gsv".to_string()),
        local: false,
    };
    assert_eq!(
        plan.command(&RebuildMode::Switch, None).to_string(),
        "nixos-rebuild switch --flake .?submodules=1#web --target-host paki@web --build-host gsv --sudo --use-substitutes"
    );
    // Already built here, so only copied to the target and activated there
    assert_eq!(
        plan.command(&RebuildMode::Boot, Some("/nix/store/abc-nixos-system-web"))
            .to_string(),
        "nixos-rebuild boot --store-path /nix/store/abc-nixos-system-web --target-host paki@web --sudo --use-substitutes"
    );
    // Built here, so with the builders
    assert_eq!(
        Plan {
            build_host: None,
            ..plan.clone()
        }
        .command(&RebuildMode::Switch, None)
        .to_string(),
        "nixos-rebuild switch --flake .?submodules=1#web --target-host paki@web --builders ssh-ng://gsv --option builders-use-substitutes true --sudo --use-substitutes"
    );
    // Building does not touch the target
    assert_eq!(
        Plan {
            local: true,
            ..plan
        }
        .command(&RebuildMode::Build, None)
        .to_string(),
        "nixos-rebuild build --flake .?submodules=1#web --build-host gsv --sudo --use-substitutes"
    );
}

#[test]
fn test_retry_command() {
    let flags = Flags {
        ask_sudo_password: true,
        use_remote_sudo: true,
        target_port: Some(2222),
        builders: &[
            "gsv".to_string(),
            "ssh-ng://big x86_64-linux - 8".to_string(),
        ],
        parallel: Some(8),
        ..Flags::default()
    };
    assert_eq!(
        retry_command(&["CL-NIX-1", "CL-ROU"], &RebuildMode::Test, &flags).join(" "),
        "snow rebuild CL-NIX-1 CL-ROU -m test -a -p 2222 -B gsv -B 'ssh-ng://big x86_64-linux - 8' -P 8"
    );
    assert_eq!(
        retry_command(
            &["web"],
            &RebuildMode::Switch,
            &Flags {
                build_host: Some(""),
                ..Flags::default()
            }
        )
        .join(" "),
        "snow rebuild web -m switch -b ''"
    );
    // A single host can have its target overridden
    assert_eq!(
        retry_command(
            &["web"],
            &RebuildMode::Boot,
            &Flags {
                target_host: Some("root@10.0.0.2"),
                ..Flags::default()
            }
        )
        .join(" "),
        "snow rebuild web -m boot -t root@10.0.0.2"
    );
}

#[test]
fn test_plan_builders() {
    let config = SnowConfig {
        tags: Vec::new(),
        use_remote_sudo: false,
        ask_sudo_password: None,
        use_substitutes: false,
        target_host: Some("paki@web".to_string()),
        build_host: None,
        build_me_on: Some("gsv".to_string()),
        target_port: None,
        parallel: 4,
        vm: None,
    };
    let plan_with = |builders: &[String], machines| {
        let flags = Flags {
            builders,
            ..Flags::default()
        };
        plan(
            "web",
            &config,
            "hub",
            || Some("px-1".to_string()),
            &flags,
            machines,
        )
    };
    // Without builders, buildMeOn and the running host's buildHost conflict
    assert!(plan_with(&[], None).is_err());
    // With them, it is built here and on them, whatever its snow config says
    let plan = plan_with(&["gsv".to_string()], Some("ssh-ng://gsv")).unwrap();
    assert_eq!(plan.build_host, None);
    assert_eq!(plan.builders.as_deref(), Some("ssh-ng://gsv"));
}

#[test]
fn test_builders() {
    let configs = BTreeMap::from([
        (
            "gsv".to_string(),
            Some(BuilderConfig {
                max_jobs: Some(16),
                supported_features: vec!["kvm".to_string(), "big-parallel".to_string()],
            }),
        ),
        (
            "bare".to_string(),
            Some(BuilderConfig {
                max_jobs: None,
                supported_features: Vec::new(),
            }),
        ),
        ("web".to_string(), None),
    ]);
    assert_eq!(builders(&[], &configs).unwrap(), None);
    assert_eq!(
        builders(
            &[
                "gsv".to_string(),
                "root@gsv".to_string(),
                "other".to_string(),
                "ssh://big x86_64-linux - 8".to_string()
            ],
            &configs
        )
        .unwrap()
        .as_deref(),
        Some(
            "ssh-ng://gsv - - 16 1 kvm,big-parallel ; ssh-ng://root@gsv - - 16 1 kvm,big-parallel ; \
             ssh-ng://other - - - 1 - ; ssh://big x86_64-linux - 8"
        )
    );
    // Without a job count or features, nix's defaults
    assert_eq!(
        builders(&["bare".to_string()], &configs)
            .unwrap()
            .as_deref(),
        Some("ssh-ng://bare - - - 1 -")
    );
    // A host of the flake must be a builder to be used as one
    assert!(builders(&["web".to_string()], &configs).is_err());
}

#[test]
fn test_listing() {
    assert_eq!(listing(&[] as &[&str]), "");
    assert_eq!(listing(&["gsv"]), "gsv");
    assert_eq!(listing(&["gsv", "abc"]), "gsv and abc");
    assert_eq!(listing(&["gsv", "abc", "here"]), "gsv, abc and here");
}

#[test]
fn test_on_build_hosts() {
    let counts = [("gsv", 1), ("nonexistent.invalid", 1)];
    assert_eq!(
        on_build_hosts(&counts, None),
        "1 on gsv and 1 on nonexistent.invalid"
    );
    assert_eq!(
        on_build_hosts(&counts, Some("host")),
        "1 host on gsv and 1 on nonexistent.invalid"
    );
    assert_eq!(
        on_build_hosts(&[("gsv", 2)], Some("host")),
        "2 hosts on gsv"
    );
}

#[test]
fn test_builder_name() {
    assert_eq!(builder_name("gsv"), "gsv");
    assert_eq!(builder_name("root@gsv"), "gsv");
    assert_eq!(
        builder_name("ssh-ng://root@gsv?compress=true x86_64-linux - 8"),
        "gsv"
    );
}

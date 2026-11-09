mod checked;
mod progress;
mod silent;
mod verbose;
mod with_return;

pub(super) use progress::{Deploy, Job, JobResult, Live};

use std::fmt::Display;
use std::process::Command;

pub(super) struct SnowCommand {
    command: String,
    args: Vec<String>,
    envs: Vec<(String, String)>,
    pub(super) requires_sudo: bool,
}

impl SnowCommand {
    pub(crate) fn new(command: String, args: Vec<&str>, requires_sudo: bool) -> Self {
        Self {
            command,
            args: args.iter().map(|x| x.to_string()).collect(),
            envs: Vec::new(),
            requires_sudo,
        }
    }

    pub(crate) fn new_nix(command: String, args: Vec<&str>, requires_sudo: bool) -> Self {
        Self::new(command, args, requires_sudo)
    }

    pub(crate) fn new_git(command: String, args: Vec<&str>) -> Self {
        Self::new(command, args, false)
    }

    pub(crate) fn new_agenix(command: String, args: Vec<&str>) -> Self {
        Self::new(command, args, false)
    }

    pub(crate) fn append_arg(&mut self, arg: &str) {
        self.args.push(arg.to_string());
    }

    pub(crate) fn set_env(&mut self, key: &str, value: &str) {
        self.envs.push((key.to_string(), value.to_string()));
    }

    fn process(&self) -> Command {
        let mut process = match self.requires_sudo {
            true => {
                let mut process = Command::new("sudo");
                process.arg(&self.command);
                process
            }
            false => Command::new(&self.command),
        };
        process.args(&self.args).envs(self.envs.clone());
        process
    }

    fn log(&self) {
        let style = anstyle::AnsiColor::Cyan.on_default();
        log::debug!("Running command: {style}{}{style:#}", self);
    }
}

fn nix_error(line: &str) -> String {
    line.replace("error:", "")
        .replace("Definition values:", "")
        .trim()
        .trim_start_matches('>')
        .trim()
        .to_string()
}

impl Display for SnowCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let combined_args = self.args.join(" ");
        let sudo = match self.requires_sudo {
            true => "sudo ",
            false => "",
        };

        write!(f, "{}{} {}", sudo, self.command, combined_args)
    }
}

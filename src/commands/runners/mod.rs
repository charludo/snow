mod interactive;
mod progress;
mod silent;
mod verbose;
mod with_return;

use std::fmt::Display;

pub(super) struct SnowCommand {
    command: String,
    args: Vec<String>,
    pub(super) requires_sudo: bool,
}

impl SnowCommand {
    pub(crate) fn new(command: String, args: Vec<&str>, requires_sudo: bool) -> Self {
        Self {
            command,
            args: args.iter().map(|x| x.to_string()).collect(),
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

    fn get_final_args(&self) -> (String, Vec<String>) {
        if self.requires_sudo {
            let mut args = self.args.clone();
            args.insert(0, self.command.clone());
            ("sudo".to_string(), args)
        } else {
            (self.command.clone(), self.args.clone())
        }
    }

    fn log(&self) {
        let style = anstyle::AnsiColor::Cyan.on_default();
        log::debug!("Running command: {style}{}{style:#}", self);
    }
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

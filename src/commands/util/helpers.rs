use crate::{commands::runners::SnowCommand, util::Result};

pub(crate) fn wrap(arg: &str, with_submodules: bool) -> String {
    match with_submodules {
        true => format!(".?submodules=1#{}", arg),
        false => format!(".#{}", arg),
    }
}

pub(crate) fn read_from_repl(attr: &str, extra_args: Vec<&str>) -> Result<String> {
    let wrapped_attr = wrap(attr, true);
    let mut args: Vec<&str> = vec!["eval", &wrapped_attr];
    args.extend_from_slice(&extra_args);
    let command = SnowCommand::new_nix("nix".to_string(), args, false);
    command.run_with_return()
}

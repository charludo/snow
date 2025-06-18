use crate::util::Result;

use super::{runners::SnowCommand, util::read_from_repl};

pub(crate) fn repl() -> Result<()> {
    let command = SnowCommand::new_nix(
        "nix".to_string(),
        vec!["repl", "--expr", "builtins.getFlake (toString ./.)"],
        false,
    );
    command.run_interactive()?;
    Ok(())
}

pub(crate) fn eval(expression: &str, json: bool, raw: bool) -> Result<()> {
    let mut extra_args = vec![];
    if json {
        extra_args.push("--json");
    }
    if raw {
        extra_args.push("--raw");
    }

    match read_from_repl(expression, extra_args) {
        Ok(result) => {
            log::info!("Result:\n{result}");
            Ok(())
        }
        Err(e) => Err(e),
    }
}

#[test]
fn test_eval() {
    testing_logger::setup();
    let _ = eval("nixosConfigurations.hostname.config", true, false);
    crate::test_util::ensure_output(
        "nix eval .?submodules=1#nixosConfigurations.hostname.config --json",
    );
}

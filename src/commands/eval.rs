use crate::util::Result;

use super::{snow_command::SnowCommand, util::read_from_repl};

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

// #[test]
// fn test_repl() {
//     testing_logger::setup();
//     let _ = repl();
//     testing_logger::validate(|captured_logs| {
//         assert_eq!(captured_logs.len(), 1);
//         assert_eq!(
//             captured_logs[0].body,
//             crate::test_util::wrap_like_logger("nix repl --expr builtins.getFlake (toString ./.)")
//         );
//         assert_eq!(captured_logs[0].level, log::Level::Debug);
//     });
// }

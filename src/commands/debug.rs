use crate::util::Result;
use gethostname::gethostname;

use super::runners::SnowCommand;
use super::util::wrap;

pub(crate) fn debug_build(nixos_configuration: &Option<String>) -> Result<()> {
    let hostname = gethostname().into_string().unwrap_or_default();
    let nixos_configuration = match nixos_configuration {
        Some(c) => c.as_str(),
        None => hostname.as_str(),
    };

    // Abort on the first evaluation warning so --show-trace points directly at it
    unsafe { std::env::set_var("NIX_ABORT_ON_WARN", "1") };

    let args = [
        "build".to_string(),
        "--flake".to_string(),
        wrap(nixos_configuration, true),
        "--show-trace".to_string(),
        "--impure".to_string(),
    ];

    let command = SnowCommand::new_nix(
        "nixos-rebuild".to_string(),
        args.iter().map(|x| x.as_str()).collect(),
        false,
    );
    command.run_verbose()?;
    Ok(())
}

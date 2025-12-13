use crate::util::Result;

use super::runners::SnowCommand;

fn bump(sed: &str) -> Result<()> {
    let command = SnowCommand::new(
        "find".to_string(),
        vec![".", "-type", "f", "-exec", "sed", "-i", sed, "{}", "+"],
        false,
    );
    command.run_verbose()?;

    Ok(())
}

pub(crate) fn bump_python(version: &str) -> Result<()> {
    bump(&format!(
        r"s/python3[1-9][1-9]\+/python{}/g",
        version.replace(".", "")
    ))
}

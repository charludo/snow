use crate::util::Result;

use super::snow_command::SnowCommand;

pub(super) fn exist_untracked() -> Result<bool> {
    Ok(SnowCommand::new_git(
        "git".to_string(),
        vec!["status", "--porcelain=v1", "--untracked-files=all"],
    )
    .run_with_return()?
    .contains("??")
        || SnowCommand::new_git(
            "git".to_string(),
            vec![
                "submodule",
                "foreach",
                "git",
                "status",
                "--porcelain=v1",
                "--untracked-files=all",
            ],
        )
        .run_with_return()?
        .contains("??"))
}

pub(crate) fn git_add() -> Result<()> {
    SnowCommand::new_git(
        "git".to_string(),
        vec!["submodule", "foreach", "git", "add", "."],
    )
    .run_silent()?;
    SnowCommand::new_git("git".to_string(), vec!["add", "."]).run_silent()?;
    Ok(())
}

pub(crate) fn git_commit(message: &Option<String>) -> Result<()> {
    let extra_args = match message {
        Some(message) => vec![message.as_str()],
        None => vec!["--amend", "-C", "HEAD"],
    };

    git_add()?;
    let mut args = vec!["submodule", "foreach", "git", "commit"];
    args.extend(extra_args.clone());
    SnowCommand::new_git("git".to_string(), args).run_silent()?;
    git_add()?;
    let mut args = vec!["commit"];
    args.extend(extra_args);
    SnowCommand::new_git("git".to_string(), args).run_silent()?;
    Ok(())
}

pub(crate) fn git_push() -> Result<()> {
    SnowCommand::new_git(
        "git".to_string(),
        vec!["submodule", "foreach", "git", "push", "--force-with-lease"],
    )
    .run_silent()?;
    SnowCommand::new_git("git".to_string(), vec!["push", "--force-with-lease"]).run_silent()?;
    Ok(())
}

pub(crate) fn git_all(message: &Option<String>) -> Result<()> {
    git_commit(message)?;
    git_push()?;
    Ok(())
}

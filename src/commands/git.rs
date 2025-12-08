use crate::util::Result;

use super::runners::SnowCommand;

pub(super) fn exist_untracked_secrets() -> Result<bool> {
    Ok(SnowCommand::new_git(
        "git".to_string(),
        vec!["status", "--porcelain=v1", "--untracked-files=all"],
    )
    .run_with_return()?
    .contains("??"))
}

pub(super) fn exist_untracked() -> Result<bool> {
    Ok(exist_untracked_secrets()?
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

pub(crate) fn git_pull(submodules_only: bool) -> Result<()> {
    SnowCommand::new_git(
        "git".to_string(),
        vec!["submodule", "foreach", "git", "pull"],
    )
    .run_silent()?;
    if submodules_only {
        return Ok(());
    }
    SnowCommand::new_git("git".to_string(), vec!["pull"]).run_silent()?;
    Ok(())
}

pub(crate) fn git_add(submodules_only: bool) -> Result<()> {
    SnowCommand::new_git(
        "git".to_string(),
        vec!["submodule", "foreach", "git", "add", "."],
    )
    .run_silent()?;
    if submodules_only {
        return Ok(());
    }
    SnowCommand::new_git("git".to_string(), vec!["add", "."]).run_silent()?;
    Ok(())
}

pub(crate) fn git_commit(message: &Option<String>, submodules_only: bool) -> Result<()> {
    let extra_args: Vec<&str> = match message {
        Some(msg) => vec!["-m", msg.as_str()],
        None => vec!["--amend", "-C", "HEAD"],
    };
    git_add(submodules_only)?;
    let mut args = vec!["submodule", "foreach", "git", "commit"];
    args.extend(extra_args.clone());
    SnowCommand::new_git("git".to_string(), args).run_silent()?;
    if submodules_only {
        return Ok(());
    }

    git_add(submodules_only)?;
    let mut args = vec!["commit"];
    args.extend(extra_args);
    SnowCommand::new_git("git".to_string(), args).run_silent()?;
    Ok(())
}

pub(crate) fn git_push(submodules_only: bool) -> Result<()> {
    SnowCommand::new_git(
        "git".to_string(),
        vec!["submodule", "foreach", "git", "push", "--force-with-lease"],
    )
    .run_silent()?;
    if submodules_only {
        return Ok(());
    }
    SnowCommand::new_git("git".to_string(), vec!["push", "--force-with-lease"]).run_silent()?;
    Ok(())
}

pub(crate) fn git_all(message: &Option<String>, submodules_only: bool) -> Result<()> {
    git_commit(message, submodules_only)?;
    git_push(submodules_only)?;
    Ok(())
}

pub(crate) fn git_init(_submodules_only: bool) -> Result<()> {
    SnowCommand::new_git("git".to_string(), vec!["submodule", "init"]).run_silent()?;
    SnowCommand::new_git("git".to_string(), vec!["submodule", "update"]).run_silent()?;
    Ok(())
}

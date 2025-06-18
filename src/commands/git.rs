use crate::util::Result;

use super::runners::SnowCommand;

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

pub(crate) fn git_pull() -> Result<()> {
    SnowCommand::new_git(
        "git".to_string(),
        vec!["submodule", "foreach", "git", "pull"],
    )
    .run_silent()?;
    SnowCommand::new_git("git".to_string(), vec!["pull"]).run_silent()?;
    Ok(())
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
    let message_string;
    let extra_args: Vec<&str> = match message {
        Some(msg) => {
            message_string = format!("\"{}\"", msg);
            vec!["-m", &message_string]
        }
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

pub(crate) fn git_init() -> Result<()> {
    SnowCommand::new_git("git".to_string(), vec!["submodule", "init"]).run_silent()?;
    SnowCommand::new_git("git".to_string(), vec!["submodule", "update"]).run_silent()?;
    Ok(())
}

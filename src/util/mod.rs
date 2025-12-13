mod args;
mod error_handling;
mod logging;

pub(super) use args::{AgenixSubcommands, Args, BumpSubcommands, Commands, GitSubcommands};
pub(super) use error_handling::{Result, SnowError};
pub(super) use logging::setup_logger;

#[cfg(test)]
pub mod test_util {
    pub(crate) fn wrap_like_logger(message: &str) -> String {
        format!("Running command: \u{1b}[36m{message}\u{1b}[0m")
    }

    pub(crate) fn ensure_output(output: &str) {
        testing_logger::validate(|captured_logs| {
            assert_eq!(captured_logs.len(), 1);
            assert_eq!(
                captured_logs[0].body,
                crate::test_util::wrap_like_logger(output)
            );
            assert_eq!(captured_logs[0].level, log::Level::Debug);
        });
    }
}

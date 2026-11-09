use clap::ValueEnum;
use strum::Display;

#[derive(ValueEnum, Debug, Display, Clone)]
#[clap(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum RebuildMode {
    Switch,
    Test,
    Boot,
    Build,
}

impl RebuildMode {
    pub fn builds_only(&self) -> bool {
        matches!(self, RebuildMode::Build)
    }
}

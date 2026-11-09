use log::LevelFilter;
use std::io::Write;

pub(crate) fn setup_logger(level: LevelFilter) {
    env_logger::builder()
        .filter(None, level)
        .format(|buf, record| {
            let style = buf.default_level_style(record.level());
            writeln!(
                buf,
                "{style}[❄ {}]{style:#} - {}",
                record.level(),
                record.args()
            )
        })
        .init();

    log::trace!("Set up logging.");
}

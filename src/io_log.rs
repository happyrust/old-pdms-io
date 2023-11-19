
use anyhow::{Context, Result};
pub fn init_log(level: log::LevelFilter) -> Result<()> {
    let local_level = level;
    fern::Dispatch::new()
        .format(move |out, message, record| {
            if local_level > log::LevelFilter::Info {
                // Add some extra info to each message in debug
                out.finish(format_args!(
                    "[{}]({})({}) {}",
                    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f"),
                    record.target(),
                    record.level(),
                    message
                ))
            } else {
                out.finish(format_args!("{}", message))
            }
        })
        .level(level)
        .chain(std::io::stdout())
        .apply()
        .context("Unable to initialize log")?;
    Ok(())
}
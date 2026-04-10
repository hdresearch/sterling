use std::path::Path;

use crate::{log, warn};

use crate::{
    error::GetSourcesError,
    ini::parse_ini,
    source::source::{Source, parse_source_name},
};

pub fn get_all_local_sources(config_dir: impl AsRef<Path>) -> Result<Vec<Source>, GetSourcesError> {
    let config_dir = config_dir.as_ref();
    let mut sources: Vec<Source> = Vec::new();

    for entry_result in config_dir.read_dir()? {
        let entry = entry_result?;
        if !entry.file_type()?.is_file() {
            warn!("Non-file entry in config dir: {}", entry.path().display());
            continue;
        }

        let file_name = entry.file_name().to_string_lossy().to_string();
        if !file_name.ends_with(".ini") {
            continue;
        }

        let source_name = &file_name[..file_name.len().saturating_sub(4)];
        let (priority, description) = parse_source_name(source_name)?;

        let source_contents = std::fs::read_to_string(entry.path())?;
        let dictionary = parse_ini(source_contents)?;

        sources.push(Source {
            priority,
            description,
            parse_result: dictionary,
        });
        log!("Successfully parsed local config source {source_name}");
    }
    Ok(sources)
}

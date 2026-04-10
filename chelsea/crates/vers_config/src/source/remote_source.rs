use std::path::Path;

use crate::log;

use crate::{
    error::GetSourcesError,
    source::{directive::parse_sources_txt, source::Source},
};

const SOURCES_TXT: &str = "sources.txt";

pub fn get_all_remote_sources(
    config_dir: impl AsRef<Path>,
) -> Result<Vec<Source>, GetSourcesError> {
    let config_dir = config_dir.as_ref();

    // Read and parse sources.txt
    let sources_txt = std::fs::read_to_string(config_dir.join(SOURCES_TXT))?;
    let directives = parse_sources_txt(sources_txt)?;

    // Execute the directive based on type
    let mut sources = Vec::new();
    for directive in directives {
        let source = directive.execute()?;
        log!(
            "Successfully parsed remote config source: {}",
            source.source_name()
        );
        sources.push(source);
    }

    Ok(sources)
}

use std::{collections::HashMap, path::Path};

use crate::log;

use crate::{error::GetSourcesError, source::source::get_all_sources_sorted};

/// Get all sources, then combine them into a single dictionary, obeying the override order described in the spec. This includes env var overrides.
pub fn get_config_dictionary(
    config_dir: impl AsRef<Path>,
) -> Result<HashMap<String, String>, GetSourcesError> {
    if !config_dir.as_ref().exists() {
        return Err(GetSourcesError::ConfigDirDoesntExist(
            config_dir.as_ref().to_path_buf().display().to_string(),
        ));
    }

    let sources = get_all_sources_sorted(config_dir)?;

    // Temporarily track the source name from which each value is derived
    let mut config_dictionary_with_source = HashMap::new();

    for source in sources {
        let source_name = source.source_name();
        for (key, value) in source.parse_result {
            config_dictionary_with_source.insert(key, (value, source_name.clone()));
        }
    }

    // Apply environment variable overrides
    const SOURCE_ENV: &str = "environment";
    for (key, (value, source_name)) in config_dictionary_with_source.iter_mut() {
        if let Ok(env_value) = std::env::var(key) {
            *value = env_value;
            *source_name = SOURCE_ENV.to_string();
        }
    }

    let mut config_dictionary_with_source_sorted: Vec<_> =
        config_dictionary_with_source.into_iter().collect();
    config_dictionary_with_source_sorted.sort_by(|a, b| a.0.cmp(&b.0));

    // Log the key/value pair plus the derived source at debug level, then drop the source name
    let config_dictionary = config_dictionary_with_source_sorted
        .into_iter()
        .map(|(key, (value, source_name))| {
            log!("{key} from {source_name}");
            (key, value)
        })
        .collect();

    Ok(config_dictionary)
}

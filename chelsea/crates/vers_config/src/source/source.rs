use std::{collections::HashMap, path::Path};

use crate::{
    error::{GetSourcesError, ParseSourceNameError},
    source::{local_source::get_all_local_sources, remote_source::get_all_remote_sources},
};

#[derive(Debug)]
pub struct Source {
    /// Source's priority; sources with greater priority override sources with lower.
    pub priority: u32,
    /// Source's description; for display purposes
    pub description: String,
    /// Dictionary containing the source's key-value pairs
    pub parse_result: HashMap<String, String>,
}

impl Source {
    pub fn source_name(&self) -> String {
        format!("{}-{}", self.priority, self.description)
    }
}

/// Parses the input string as a source name, returning the priority and description
pub fn parse_source_name<S: AsRef<str>>(
    source_name: S,
) -> Result<(u32, String), ParseSourceNameError> {
    let source_name = source_name.as_ref();

    let dash_pos = source_name
        .find('-')
        .ok_or(ParseSourceNameError::MissingDash {
            source_name: source_name.to_string(),
        })?;

    let priority_str = &source_name[..dash_pos];
    let description = &source_name[dash_pos + 1..];

    if description.is_empty() {
        return Err(ParseSourceNameError::EmptyDescription {
            source_name: source_name.to_string(),
        });
    }

    let priority =
        priority_str
            .parse::<u32>()
            .map_err(|_| ParseSourceNameError::InvalidPriority {
                source_name: source_name.to_string(),
            })?;

    Ok((priority, description.to_string()))
}

/// Return a list of all sources, sorted by priority in ascending order
pub fn get_all_sources_sorted(
    config_dir: impl AsRef<Path>,
) -> Result<Vec<Source>, GetSourcesError> {
    let mut sources = get_all_local_sources(&config_dir)?;
    sources.extend(get_all_remote_sources(config_dir)?);
    sources.sort_by(|a, b| a.priority.cmp(&b.priority));
    for pair in sources.windows(2) {
        if pair[0].priority == pair[1].priority {
            return Err(GetSourcesError::NonUniquePriority(
                pair[0].source_name(),
                pair[1].source_name(),
            ));
        }
    }
    Ok(sources)
}

use std::collections::HashMap;

use crate::error::ParseIniError;

/// Given the file contents of an ini file, parse it into a key-value dictionary
pub fn parse_ini<S: AsRef<str>>(ini_str: S) -> Result<HashMap<String, String>, ParseIniError> {
    let ini_str = ini_str.as_ref();
    let mut map = HashMap::new();

    for (line_num, line) in ini_str.lines().enumerate() {
        let line = line.trim();

        // Skip comments, sections, and empty lines
        if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
            continue;
        }

        // Find the '=' separator
        let eq_pos = line
            .find('=')
            .ok_or_else(|| ParseIniError::MissingEquals { line: line_num + 1 })?;

        let key = line[..eq_pos].trim();
        let value = line[eq_pos + 1..].trim();

        // Validate key format
        if !key
            .chars()
            .all(|c| c.is_ascii_alphabetic() || c.is_ascii_alphanumeric() || c == '_')
        {
            return Err(ParseIniError::InvalidKey {
                line: line_num + 1,
                key: key.to_string(),
            });
        }

        if key.is_empty() {
            return Err(ParseIniError::EmptyKey { line: line_num + 1 });
        }

        // Case-fold key to lowercase
        let key_lower = key.to_lowercase();

        map.insert(key_lower, value.to_string());
    }

    Ok(map)
}

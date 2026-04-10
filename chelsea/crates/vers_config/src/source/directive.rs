use std::str::FromStr;

use util::secrets::read_secret_string;

use crate::{
    error::{ExecuteDirectiveError, ParseDirectiveError, ParseSourcesTxtError},
    ini::parse_ini,
    source::source::{Source, parse_source_name},
};

/// Represents a parsed line of the sources.txt file
pub enum SourceDirective {
    AwsSecret {
        source_name: String,
        secret_id: String,
    },
}

impl SourceDirective {
    pub fn source_name<'a>(&'a self) -> &'a str {
        match self {
            Self::AwsSecret {
                source_name,
                secret_id: _,
            } => source_name.as_str(),
        }
    }

    /// Execute the directive, returning a Source if successful
    pub fn execute(&self) -> Result<Source, ExecuteDirectiveError> {
        let (priority, description) = parse_source_name(self.source_name())?;

        match self {
            Self::AwsSecret {
                secret_id,
                source_name: _,
            } => {
                let secret_content = read_secret_string(&secret_id)?;
                let parse_result = parse_ini(secret_content)?;

                Ok(Source {
                    priority,
                    description,
                    parse_result,
                })
            }
        }
    }
}

impl FromStr for SourceDirective {
    type Err = ParseDirectiveError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split_whitespace().collect();

        if parts.is_empty() {
            return Err(ParseDirectiveError::EmptyDirective);
        }

        let directive_command = parts[0];

        match directive_command {
            "aws-secret" => {
                if parts.len() != 3 {
                    return Err(ParseDirectiveError::InvalidArgumentCount {
                        directive: "aws-secret".to_string(),
                        expected: 2,
                        found: parts.len() - 1,
                    });
                }

                Ok(SourceDirective::AwsSecret {
                    source_name: parts[1].to_string(),
                    secret_id: parts[2].to_string(),
                })
            }
            _ => Err(ParseDirectiveError::UnknownDirective {
                directive: directive_command.to_string(),
            }),
        }
    }
}

/// Parses the given sources_txt string, returning a list of source directives on success
pub fn parse_sources_txt(
    sources_txt: impl AsRef<str>,
) -> Result<Vec<SourceDirective>, ParseSourcesTxtError> {
    let mut directives = Vec::new();
    for (line_number, line) in sources_txt.as_ref().lines().enumerate() {
        let line = line.trim();

        // Skip empty lines
        if line.is_empty() {
            continue;
        }

        // Parse the directive
        let directive: SourceDirective = line
            .parse()
            .map_err(|error| ParseSourcesTxtError::ParseLineError { line_number, error })?;
        directives.push(directive);
    }
    Ok(directives)
}

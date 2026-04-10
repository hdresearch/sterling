mod config_dictionary;
mod error;
mod ini;
mod logging;
mod source;
mod vers_config;

pub use vers_config::HypervisorType;
pub use vers_config::VersConfig;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_config_dictionary() {
        let config = VersConfig::global();
        println!("{config:?}");
    }
}

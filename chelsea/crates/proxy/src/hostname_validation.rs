use std::str::FromStr;

use uuid::Uuid;

use vers_config::VersConfig;

#[derive(Debug)]
pub enum VmToHostnameError {
    /// The hostname string was badly formatted. Can be:
    /// - invalid uuid when expected, for example: <invalid uuid>.vm.vers.sh
    BadFormat,
    /// domain or VM not found in pg.
    NotFound,
    /// Error when calling pg. from [crate::pg]
    DB(anyhow::Error),
    WG(orch_wg::WireguardInterfaceError),
}

/// Which hostname the SNI is pointing to. Should **ONLY** be used for selecting
/// TLS certs. Any routing on HTTP level should be done by "Host" header:
/// https://www.rfc-editor.org/rfc/rfc9110?referrer=grok.com#section-7.2
///
/// This type is not guarranteed to hold valid values.
#[derive(Clone, Debug)]
pub enum SniEndpoint {
    /// Custom domain that maybe points to an existing VM. 'String' is domain.
    VmWithCustomHostname(String),
    /// User requests to a Vm using our domains: `<vm_id>.vm.vers.sh`
    /// Can be ssh or https. Vm uuid doesn't have to exist.
    Vm(Uuid),
    /// hostname: `api.vers.sh`
    VersApi,
}

pub enum ParseHostError {
    SniAndHostHeaderNotMatching,
    InvalidHost,
}

fn strip_port(host_header: String) -> String {
    host_header.split(":").next().unwrap_or("").to_string()
}

pub fn parse_host(host_header: String) -> Option<HostHeaderEndpoint> {
    if host_header.is_empty() {
        tracing::warn!("HOST header is empty");
        return None;
    }

    // Normalize to lowercase for case-insensitive comparison (DNS is
    // case-insensitive per RFC 1035)
    let host_header = host_header.to_ascii_lowercase();

    let host_header = strip_port(host_header);

    // orch_host is as of writing 'api.vers.sh'
    if host_header == format!("api.{}", VersConfig::orchestrator().host) {
        return Some(HostHeaderEndpoint::VersApi);
    }

    let parsed = match host_header.strip_suffix(&format!(".vm.{}", VersConfig::orchestrator().host))
    {
        Some(maybe_vm_id) => match Uuid::from_str(maybe_vm_id) {
            Ok(valid_uuid) => SniEndpoint::Vm(valid_uuid),
            Err(err) => {
                tracing::warn!(error = %&err, "vm id or custom domain badly formatted");
                return None;
            }
        },
        None => {
            if hostname_is_valid(&host_header) {
                SniEndpoint::VmWithCustomHostname(host_header)
            } else {
                tracing::warn!(%host_header,  "invalid hostname");
                return None;
            }
        }
    };

    let host = match parsed {
        SniEndpoint::Vm(vm) => HostHeaderEndpoint::Vm(vm),
        SniEndpoint::VersApi => HostHeaderEndpoint::VersApi,
        SniEndpoint::VmWithCustomHostname(host) => HostHeaderEndpoint::VmWithCustomHostname(host),
    };

    Some(host)
}

pub fn parse_host_and_validate_sni(
    host_header: String,
    sni: SniEndpoint,
) -> Result<HostHeaderEndpoint, ParseHostError> {
    let Some(parsed) = parse_host(host_header) else {
        return Err(ParseHostError::InvalidHost);
    };

    // Make sure host_header is same as SNI.
    match (parsed, sni) {
        (HostHeaderEndpoint::VersApi, SniEndpoint::VersApi) => Ok(HostHeaderEndpoint::VersApi),
        (HostHeaderEndpoint::Vm(vm1), SniEndpoint::Vm(vm2)) if vm1 == vm2 => {
            Ok(HostHeaderEndpoint::Vm(vm1))
        }
        (
            HostHeaderEndpoint::VmWithCustomHostname(hostname1),
            SniEndpoint::VmWithCustomHostname(hostname2),
        ) if hostname1 == hostname2 => Ok(HostHeaderEndpoint::VmWithCustomHostname(hostname1)),
        (_, _) => Err(ParseHostError::SniAndHostHeaderNotMatching),
    }
}

/// Which hostname the "Host" header is pointing to. This should be used for
/// routing per RFC:
/// https://www.rfc-editor.org/rfc/rfc9110?referrer=grok.com#section-7.2
///
/// Don't create this outside of this module!!
///
/// This type is not guarranteed to hold valid values.
#[derive(Debug)]
pub enum HostHeaderEndpoint {
    /// Custom domain that maybe points to an existing VM. 'String' is domain.
    VmWithCustomHostname(String),
    /// User requests to a Vm using our domains: `<vm_id>.vm.vers.sh`
    /// Can be ssh or https. Vm uuid doesn't have to exist.
    Vm(Uuid),
    /// hostname: `api.vers.sh`
    VersApi,
}

/// This only parses SNI, it doesn't validate any resources behind these hostnames.
/// Normalizes the hostname to lowercase since DNS is case-insensitive (RFC 1035).
pub fn parse_sni(sni: String) -> Option<SniEndpoint> {
    if sni.is_empty() {
        tracing::warn!("sni is empty");
        return None;
    }

    // Normalize to lowercase for case-insensitive comparison (DNS is case-insensitive per RFC 1035)
    let sni = sni.to_ascii_lowercase();

    // orch_host is as of writing 'api.vers.sh'
    if sni == format!("api.{}", VersConfig::orchestrator().host) {
        return Some(SniEndpoint::VersApi);
    }

    match sni.strip_suffix(&format!(".vm.{}", VersConfig::orchestrator().host)) {
        Some(maybe_vm_id) => match Uuid::from_str(maybe_vm_id) {
            Ok(valid_uuid) => Some(SniEndpoint::Vm(valid_uuid)),
            Err(err) => {
                tracing::warn!(error = %&err, "vm id or custom domain badly formatted");
                None
            }
        },
        None => {
            if hostname_is_valid(&sni) {
                Some(SniEndpoint::VmWithCustomHostname(sni))
            } else {
                tracing::warn!(%sni,  "invalid hostname");
                None
            }
        }
    }
}

// Taken from crate: "hostname_validator"
// License: MIT
/// Validate a hostname according to [IETF RFC 1123](https://tools.ietf.org/html/rfc1123).
///
/// A hostname is valid if the following condition are true:
///
/// - It does not start or end with `-` or `.`.
/// - It does not contain any characters outside of the alphanumeric range, except for `-` and `.`.
/// - It is not empty.
/// - It is 253 or fewer characters.
/// - Its labels (characters separated by `.`) are not empty.
/// - Its labels are 63 or fewer characters.
/// - Its lables do not start or end with '-' or '.'.
pub fn hostname_is_valid(hostname: &str) -> bool {
    fn is_valid_char(byte: u8) -> bool {
        (b'a'..=b'z').contains(&byte)
            || (b'A'..=b'Z').contains(&byte)
            || (b'0'..=b'9').contains(&byte)
            || byte == b'-'
            || byte == b'.'
    }

    !(hostname.bytes().any(|byte| !is_valid_char(byte))
        || hostname.split('.').any(|label| {
            label.is_empty() || label.len() > 63 || label.starts_with('-') || label.ends_with('-')
        })
        || hostname.is_empty()
        || hostname.len() > 253)
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn valid_hostnames() {
        for hostname in &[
            "VaLiD-HoStNaMe",
            "50-name",
            "235235",
            "example.com",
            "VaLid.HoStNaMe",
            "123.456",
        ] {
            assert!(hostname_is_valid(hostname), "{} is not valid", hostname);
        }
    }

    #[test]
    fn invalid_hostnames() {
        for hostname in &[
            "-invalid-name",
            "also-invalid-",
            "asdf@fasd",
            "@asdfl",
            "asd f@",
            ".invalid",
            "invalid.name.",
            "foo.label-is-way-to-longgggggggggggggggggggggggggggggggggggggggggggg.org",
            "invalid.-starting.char",
            "invalid.ending-.char",
            "empty..label",
        ] {
            assert!(
                !hostname_is_valid(hostname),
                "{} should not be valid",
                hostname
            );
        }
    }
}

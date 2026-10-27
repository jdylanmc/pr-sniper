//! Azure CLI process boundary: discovery, environment, version and the one
//! place where Azure CLI output becomes a typed [`TokenError`]. Output text is
//! never kept, logged or shown.

use super::token_source::{AccessToken, TokenError};
use crate::cli_process::{EnvPolicy, RunError};
use serde::Deserialize;
use std::ffi::OsString;
use std::path::PathBuf;
use zeroize::Zeroizing;

pub const ENV: EnvPolicy = EnvPolicy {
    // Agent hosts set this; Azure CLI then bypasses its token cache and falls
    // back to an interactive browser sign-in on authentication failure.
    remove: &["COPILOT_AGENT_SESSION_ID"],
    // Per-process overrides; the user's Azure CLI configuration is never written.
    set: &[
        // Otherwise a detached telemetry uploader outlives the child.
        ("AZURE_CORE_COLLECT_TELEMETRY", "false"),
        ("AZURE_CORE_ONLY_SHOW_ERRORS", "true"),
        ("AZURE_AUTO-UPGRADE_ENABLE", "no"),
        ("AZURE_CORE_NO_COLOR", "true"),
    ],
};

/// On Windows Azure CLI is a batch script; `cli_process` only passes it
/// arguments that need no cmd.exe quoting.
#[cfg(windows)]
const PROGRAM: &str = "az.cmd";
#[cfg(not(windows))]
const PROGRAM: &str = "az";

/// Absolute PATH entries, then the platform install directories.
pub fn discover() -> Option<PathBuf> {
    let paths = std::env::var_os("PATH").unwrap_or_default();
    #[cfg(windows)]
    let fallback = windows_install_dirs(|name| std::env::var_os(name));
    #[cfg(not(windows))]
    let fallback = Vec::new();
    crate::process_path::find(&[PROGRAM], &paths, &fallback)
}

/// Where the Azure CLI installer (also run by winget) puts `az.cmd`. Searched
/// after PATH because a running app keeps the PATH it started with, so an
/// install from Settings is found without a restart.
#[cfg_attr(not(windows), allow(dead_code))]
fn windows_install_dirs(var: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    for name in ["ProgramFiles", "ProgramFiles(x86)"] {
        let Some(root) = var(name).filter(|root| !root.is_empty()) else {
            continue;
        };
        let dir = PathBuf::from(root)
            .join("Microsoft SDKs")
            .join("Azure")
            .join("CLI2")
            .join("wbin");
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AzVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

/// First release whose `get-access-token` reports POSIX `expires_on`.
pub const MIN_VERSION: AzVersion = AzVersion {
    major: 2,
    minor: 54,
    patch: 0,
};

impl std::fmt::Display for AzVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Parses `az version --output json`.
pub fn parse_version(stdout: &[u8]) -> Result<AzVersion, TokenError> {
    #[derive(Deserialize)]
    struct Raw {
        #[serde(rename = "azure-cli")]
        azure_cli: String,
    }
    let raw: Raw = serde_json::from_slice(stdout).map_err(|_| TokenError::MalformedOutput)?;
    let mut parts = raw.azure_cli.splitn(3, '.').map(|part| {
        let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
        digits.parse::<u32>().ok()
    });
    match (parts.next(), parts.next(), parts.next()) {
        (Some(Some(major)), Some(Some(minor)), Some(Some(patch))) => Ok(AzVersion {
            major,
            minor,
            patch,
        }),
        _ => Err(TokenError::MalformedOutput),
    }
}

pub fn run_error(error: RunError) -> TokenError {
    match error {
        RunError::NotFound => TokenError::AzMissing,
        RunError::Timeout => TokenError::Timeout,
        RunError::OutputLimit => TokenError::MalformedOutput,
        RunError::Failed => TokenError::Other {
            exit_code: None,
            aadsts: None,
        },
    }
}

/// Parses `az account get-access-token --output json`.
pub fn parse_token(stdout: &[u8]) -> Result<AccessToken, TokenError> {
    #[derive(Deserialize)]
    struct Raw {
        #[serde(rename = "accessToken")]
        access_token: String,
        #[serde(rename = "tokenType")]
        token_type: Option<String>,
        /// POSIX seconds. `expiresOn` is naive local time and is ignored.
        expires_on: Option<i64>,
    }
    let raw: Raw = serde_json::from_slice(stdout).map_err(|_| TokenError::MalformedOutput)?;
    let secret = Zeroizing::new(raw.access_token);
    let valid_secret = !secret.is_empty()
        && secret.len() <= 16_384
        && secret.bytes().all(|b| b.is_ascii_graphic());
    if !valid_secret || raw.token_type.as_deref().is_some_and(|t| t != "Bearer") {
        return Err(TokenError::MalformedOutput);
    }
    let expires_on = raw.expires_on.ok_or(TokenError::MalformedOutput)?;
    Ok(AccessToken::new(secret, expires_on))
}

fn aadsts_code(text: &str) -> Option<u32> {
    let start = text.find("AADSTS")? + "AADSTS".len();
    let digits: String = text[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// Maps a failed Azure CLI run to a typed cause. Azure CLI exits 1 for every
/// authentication failure and reports several as tracebacks, so only stable
/// markers are matched: Microsoft Entra `AADSTS` codes and fixed hints.
/// Unknown output maps to [`TokenError::Other`].
pub fn classify(exit_code: Option<i32>, stderr: &[u8]) -> TokenError {
    let text = String::from_utf8_lossy(stderr);
    match aadsts_code(&text) {
        Some(90002) => TokenError::TenantNotFound,
        Some(50020 | 50034 | 50177 | 500200) => TokenError::NoAccountInTenant,
        Some(500213) => TokenError::TenantAccessBlocked,
        Some(
            code @ (50055 | 50057 | 50072 | 50074 | 50076 | 50078 | 50079 | 50097 | 50131 | 50133
            | 50158 | 50173 | 53000 | 53001 | 53002 | 53003 | 53004 | 530003 | 65001
            | 70043 | 700082),
        ) => TokenError::InteractionRequired { aadsts: code },
        Some(code) => TokenError::Other {
            exit_code,
            aadsts: Some(code),
        },
        None if text.contains("Please run 'az login'")
            || text.contains("does not exist in MSAL token cache") =>
        {
            TokenError::NotLoggedIn
        }
        None if text.contains("Max retries exceeded")
            || text.contains("Failed to establish a new connection")
            || text.contains("NameResolutionError") =>
        {
            TokenError::Network
        }
        None => TokenError::Other {
            exit_code,
            aadsts: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_azure_cli_on_path_first_then_in_the_installer_directories() {
        let root = std::env::current_dir()
            .unwrap()
            .join(format!(".pr-sniper-az-{}", uuid::Uuid::new_v4()));
        let program_files = root.join("Program Files");
        let program_files_x86 = root.join("Program Files (x86)");
        let env = |name: &str| match name {
            "ProgramFiles" => Some(program_files.clone().into_os_string()),
            "ProgramFiles(x86)" => Some(program_files_x86.clone().into_os_string()),
            _ => None,
        };
        let wbin = |base: &PathBuf| base.join("Microsoft SDKs/Azure/CLI2/wbin");
        let dirs = windows_install_dirs(env);
        assert_eq!(dirs, [wbin(&program_files), wbin(&program_files_x86)]);
        let x86 = wbin(&program_files_x86);
        std::fs::create_dir_all(&x86).unwrap();
        std::fs::write(x86.join("az.cmd"), b"not executed").unwrap();
        // A stale PATH (no Azure CLI entry) still finds a fresh install.
        let path = std::env::join_paths([root.join("bin")]).unwrap();
        assert_eq!(
            crate::process_path::find(&["az.cmd"], &path, &dirs),
            Some(x86.join("az.cmd"))
        );
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(root.join("bin").join("az.cmd"), b"not executed").unwrap();
        assert_eq!(
            crate::process_path::find(&["az.cmd"], &path, &dirs),
            Some(root.join("bin").join("az.cmd"))
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn skips_unset_and_duplicate_program_files_directories() {
        assert!(windows_install_dirs(|_| None).is_empty());
        let same = |name: &str| match name {
            "ProgramFiles" | "ProgramFiles(x86)" => Some(OsString::from("/pf")),
            _ => Some(OsString::new()),
        };
        assert_eq!(windows_install_dirs(same).len(), 1);
        let empty_x64 = |name: &str| {
            Some(OsString::from(if name == "ProgramFiles" {
                ""
            } else {
                "/x86"
            }))
        };
        assert_eq!(
            windows_install_dirs(empty_x64),
            [PathBuf::from("/x86").join("Microsoft SDKs/Azure/CLI2/wbin")]
        );
    }

    #[test]
    fn classifies_captured_azure_cli_failures() {
        // Captured from Azure CLI 2.90; identifiers replaced with placeholders.
        let cases: [(&str, TokenError); 9] = [
            (
                "ERROR: Please run 'az login' to setup account.\n",
                TokenError::NotLoggedIn,
            ),
            (
                "ERROR: User 'someone@example.com' does not exist in MSAL token cache. Run `az login`.\n",
                TokenError::NotLoggedIn,
            ),
            (
                "ERROR: The command failed with an unexpected error. Here is the traceback:\nERROR: Unable to get authority configuration for https://login.example/00000000-0000-0000-0000-000000000009. ...\nValueError: OIDC Discovery failed ... \"error_description\":\"AADSTS90002: Tenant '00000000-0000-0000-0000-000000000009' not found.",
                TokenError::TenantNotFound,
            ),
            (
                "ERROR: AADSTS500213: The resource tenant's cross-tenant access policy does not allow this user to access this tenant. Trace ID: x\nRun the command below to authenticate interactively; additional arguments may be added as needed:\naz logout\naz login --tenant \"00000000-0000-0000-0000-000000000001\" --scope \"499b84ac-1321-427f-aa17-267ca6975798/.default\"\n",
                TokenError::TenantAccessBlocked,
            ),
            (
                "ERROR: AADSTS50020: User account from identity provider does not exist in tenant.",
                TokenError::NoAccountInTenant,
            ),
            (
                "ERROR: AADSTS50076: you must use multi-factor authentication to access '499b84ac-1321-427f-aa17-267ca6975798'.",
                TokenError::InteractionRequired { aadsts: 50076 },
            ),
            (
                "ERROR: AADSTS53003: Access has been blocked by Conditional Access policies.",
                TokenError::InteractionRequired { aadsts: 53003 },
            ),
            (
                "ERROR: HTTPSConnectionPool(host='login.example', port=443): Max retries exceeded with url: ...",
                TokenError::Network,
            ),
            (
                "ERROR: ... NameResolutionError(\"Failed to resolve 'login.example'\")",
                TokenError::Network,
            ),
        ];
        for (stderr, expected) in cases {
            assert_eq!(classify(Some(1), stderr.as_bytes()), expected, "{stderr}");
        }
    }

    #[test]
    fn unknown_failures_are_other_with_codes_only() {
        assert_eq!(
            classify(Some(2), b"ERROR: something new"),
            TokenError::Other {
                exit_code: Some(2),
                aadsts: None
            }
        );
        assert_eq!(
            classify(Some(1), b"AADSTS7000215: x"),
            TokenError::Other {
                exit_code: Some(1),
                aadsts: Some(7_000_215)
            }
        );
        assert_eq!(
            classify(None, b"AADSTS"),
            TokenError::Other {
                exit_code: None,
                aadsts: None
            }
        );
    }

    #[test]
    fn parses_versions_for_the_minimum_check() {
        let version = |text: &str| {
            parse_version(format!(r#"{{"azure-cli":"{text}","extensions":{{}}}}"#).as_bytes())
        };
        assert_eq!(
            version("2.90.0"),
            Ok(AzVersion {
                major: 2,
                minor: 90,
                patch: 0
            })
        );
        assert!(version("2.53.1").unwrap() < MIN_VERSION);
        assert!(version("2.54.0b1").unwrap() >= MIN_VERSION);
        for text in ["2.54", "x.y.z", ""] {
            assert_eq!(version(text), Err(TokenError::MalformedOutput), "{text}");
        }
        assert_eq!(parse_version(b"{}"), Err(TokenError::MalformedOutput));
    }

    #[test]
    fn rejects_malformed_token_output() {
        for stdout in [
            "not json",
            r#"{"accessToken":"x","tokenType":"Bearer"}"#,
            r#"{"accessToken":"a b","expires_on":9,"tokenType":"Bearer"}"#,
            r#"{"accessToken":"","expires_on":9}"#,
            r#"{"accessToken":"x","expires_on":9,"tokenType":"pop"}"#,
        ] {
            assert_eq!(
                parse_token(stdout.as_bytes()).unwrap_err(),
                TokenError::MalformedOutput,
                "{stdout}"
            );
        }
        let token = parse_token(
            br#"{"accessToken":"fixture.token","expiresOn":"2026-01-01 00:00:00.000000","expires_on":9,"tokenType":"Bearer"}"#,
        )
        .unwrap();
        assert_eq!((token.secret(), token.expires_on), ("fixture.token", 9));
    }
}

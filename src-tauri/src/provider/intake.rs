//! Explicit intake: one parser for a pasted pull request or repository URL,
//! dispatching by host to the provider that owns it. A new provider adds an
//! arm here; callers such as MCP `watch_pull_request` stay unchanged.

use super::ConnectionError;
use crate::storage::{ProviderId, Repository};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IntakeTarget {
    pub provider: ProviderId,
    /// GitHub `owner/repository`; Azure DevOps
    /// `{organization}/{project}/{repository}` as pasted (names or IDs).
    pub name: String,
    pub pull_request_number: Option<u64>,
}

/// Parses a GitHub (`github.com`, or bare `owner/repository`) or Azure DevOps
/// (`dev.azure.com`, `{org}.visualstudio.com`, or bare
/// `{org}/{project}/{repository}`) target.
pub fn parse(input: &str) -> Result<IntakeTarget, ConnectionError> {
    let input = input.trim();
    let host = input
        .contains("://")
        .then(|| reqwest::Url::parse(input).ok())
        .flatten()
        .and_then(|url| url.host_str().map(str::to_ascii_lowercase));
    match host.as_deref() {
        Some("github.com") => github(input),
        Some(host) if host == "dev.azure.com" || host.ends_with(".visualstudio.com") => {
            azure_devops(input)
        }
        Some(_) => Err(ConnectionError::InvalidRepository),
        None if input.contains("://") => Err(ConnectionError::InvalidRepository),
        None => github(input).or_else(|_| azure_devops(input)),
    }
}

fn github(input: &str) -> Result<IntakeTarget, ConnectionError> {
    let target = crate::github::intake::repository_target(input)?;
    Ok(IntakeTarget {
        provider: ProviderId::Github,
        name: target.name,
        pull_request_number: target.pull_request_number,
    })
}

fn azure_devops(input: &str) -> Result<IntakeTarget, ConnectionError> {
    use crate::azure_devops::read::{parse_pull_request_url, parse_repository_url};
    let (path, number) = match parse_pull_request_url(input) {
        Some((path, number)) => (path, Some(number)),
        None => (
            parse_repository_url(input).ok_or(ConnectionError::InvalidRepository)?,
            None,
        ),
    };
    Ok(IntakeTarget {
        provider: ProviderId::AzureDevops,
        name: format!("{}/{}/{}", path.organization, path.project, path.repository),
        pull_request_number: number,
    })
}

impl IntakeTarget {
    fn organization(&self) -> &str {
        self.name.split('/').next().unwrap_or_default()
    }

    /// Whether `repository` can be this target before any provider read.
    /// GitHub: the same canonical name. Azure DevOps: the same organization;
    /// project and repository are confirmed by stable ID once resolved,
    /// since a URL may carry display names or IDs.
    pub fn may_be(&self, repository: &Repository) -> bool {
        if repository.provider != self.provider {
            return false;
        }
        match self.provider {
            ProviderId::Github => repository.name == self.name,
            ProviderId::AzureDevops => repository
                .provider_repository_id
                .as_deref()
                .and_then(|id| id.split('/').next())
                .is_some_and(|organization| organization.eq_ignore_ascii_case(self.organization())),
        }
    }

    /// Whether `repository` is saved under this target's name; offline only.
    /// Azure DevOps names compare case-insensitively, as Azure DevOps does.
    pub fn names(&self, repository: &Repository) -> bool {
        repository.provider == self.provider
            && match self.provider {
                ProviderId::Github => repository.name == self.name,
                ProviderId::AzureDevops => repository.name.eq_ignore_ascii_case(&self.name),
            }
    }
}

/// The pull request URL of a saved repository, addressed by its stable
/// identity: GitHub's canonical name; Azure DevOps project and repository IDs.
pub fn pull_request_url(repository: &Repository, number: u64) -> Option<String> {
    if number == 0 {
        return None;
    }
    match repository.provider {
        ProviderId::Github => Some(format!(
            "https://github.com/{}/pull/{number}",
            repository.name
        )),
        ProviderId::AzureDevops => {
            let key = crate::azure_devops::read::RepositoryKey::parse(
                repository.provider_repository_id.as_deref()?,
            )?;
            Some(format!(
                "https://dev.azure.com/{}/{}/_git/{}/pullrequest/{number}",
                key.organization,
                key.project_id.as_str(),
                key.repository_id.as_str()
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository(provider: &str, name: &str, id: &str) -> Repository {
        serde_json::from_value(serde_json::json!({
            "id":"00000000-0000-4000-8000-000000000001","provider":provider,"name":name,
            "enabled":true,"provider_account_id":"1","provider_repository_id":id
        }))
        .unwrap()
    }

    const PROJECT: &str = "00000000-0000-0000-0000-0000000000b1";
    const REPO: &str = "00000000-0000-0000-0000-0000000000c1";

    #[test]
    fn dispatches_by_host() {
        for (input, provider, name, number) in [
            ("octo-org/app", ProviderId::Github, "octo-org/app", None),
            (
                "https://github.com/octo-org/app/pull/3",
                ProviderId::Github,
                "octo-org/app",
                Some(3),
            ),
            (
                "https://dev.azure.com/contoso/project/_git/repo/pullrequest/9?_a=files",
                ProviderId::AzureDevops,
                "contoso/project/repo",
                Some(9),
            ),
            (
                "https://contoso.visualstudio.com/DefaultCollection/My%20Project/_git/repo/pullrequest/9/",
                ProviderId::AzureDevops,
                "contoso/My Project/repo",
                Some(9),
            ),
            (
                "https://dev.azure.com/contoso/project/_git/repo",
                ProviderId::AzureDevops,
                "contoso/project/repo",
                None,
            ),
            (
                "contoso/project/repo",
                ProviderId::AzureDevops,
                "contoso/project/repo",
                None,
            ),
        ] {
            assert_eq!(
                parse(input),
                Ok(IntakeTarget {
                    provider,
                    name: name.into(),
                    pull_request_number: number
                }),
                "{input}"
            );
        }
        for input in [
            "https://example.com/contoso/project/_git/repo/pullrequest/9",
            "https://gitlab.com/octo-org/app/pull/3",
            "ftp://dev.azure.com/contoso/project/_git/repo/pullrequest/9",
            "https://dev.azure.com/contoso/project/repo/pullrequest/9",
            "not a url",
        ] {
            assert_eq!(
                parse(input),
                Err(ConnectionError::InvalidRepository),
                "{input}"
            );
        }
    }

    #[test]
    fn matches_saved_repositories_by_provider_and_identity() {
        let saved = repository(
            "azure_devops",
            "contoso/Project/Repo",
            &format!("contoso/{PROJECT}/{REPO}"),
        );
        let target =
            parse("https://Contoso.visualstudio.com/project/_git/repo/pullrequest/1").unwrap();
        assert!(target.may_be(&saved) && target.names(&saved));
        let renamed = parse("https://dev.azure.com/contoso/Other/_git/repo/pullrequest/1").unwrap();
        assert!(renamed.may_be(&saved) && !renamed.names(&saved));
        let foreign =
            parse("https://dev.azure.com/fabrikam/Project/_git/Repo/pullrequest/1").unwrap();
        assert!(!foreign.may_be(&saved) && !foreign.names(&saved));
        let github = repository("github", "octo-org/app", "301");
        let target = parse("https://github.com/octo-org/app/pull/1").unwrap();
        assert!(target.may_be(&github) && target.names(&github));
        assert!(!target.may_be(&saved) && !parse("octo-org/other").unwrap().may_be(&github));
    }

    #[test]
    fn saved_repositories_address_pull_requests_by_stable_identity() {
        let saved = repository(
            "azure_devops",
            "contoso/Project/Repo",
            &format!("contoso/{PROJECT}/{REPO}"),
        );
        let url = pull_request_url(&saved, 5).unwrap();
        assert_eq!(
            url,
            format!("https://dev.azure.com/contoso/{PROJECT}/_git/{REPO}/pullrequest/5")
        );
        assert_eq!(parse(&url).unwrap().pull_request_number, Some(5));
        assert_eq!(
            pull_request_url(&repository("github", "octo-org/app", "301"), 5).as_deref(),
            Some("https://github.com/octo-org/app/pull/5")
        );
        assert_eq!(pull_request_url(&saved, 0), None);
        assert_eq!(
            pull_request_url(&repository("azure_devops", "contoso/p/r", "bad"), 5),
            None
        );
    }
}

use super::{
    provider::{parse_response, GithubClient, RemoteRepository, Transport},
    publication::{MutationTransport, Request},
    ConnectionError,
};
use crate::{publication::WriteFailure, review::Failure};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TopComment {
    pub id: String,
    pub body: String,
    pub author_id: Option<String>,
    pub author_login: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl TopComment {
    pub fn eligible_other_user(&self, account: &str) -> bool {
        self.author_id.as_deref().is_some_and(|id| id != account)
            && !self.body.contains("<!-- pr-sniper:")
            && !self.body.trim().is_empty()
    }
    pub fn mentions(&self, login: &str, _account: &str) -> bool {
        if self.body.contains("<!-- pr-sniper:") {
            return false;
        }
        let text = self.body.to_ascii_lowercase();
        let target = format!("@{}", login.to_ascii_lowercase());
        let identity = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
        !login.is_empty()
            && text.match_indices(&target).any(|(index, _)| {
                text[..index]
                    .chars()
                    .next_back()
                    .is_none_or(|c| !identity(c) && c != '@')
                    && text[index + target.len()..]
                        .chars()
                        .next()
                        .is_none_or(identity_negated)
            })
    }
}

fn identity_negated(c: char) -> bool {
    !(c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn parse(value: &Value) -> Result<TopComment, ConnectionError> {
    let id = value["id"]
        .as_u64()
        .filter(|n| *n > 0)
        .ok_or(ConnectionError::InvalidResponse)?
        .to_string();
    let text = |key: &str| {
        value[key]
            .as_str()
            .map(str::to_string)
            .ok_or(ConnectionError::InvalidResponse)
    };
    let created_at = text("created_at")?;
    let updated_at = text("updated_at")?;
    if chrono::DateTime::parse_from_rfc3339(&created_at).is_err()
        || chrono::DateTime::parse_from_rfc3339(&updated_at).is_err()
    {
        return Err(ConnectionError::InvalidResponse);
    }
    let (author_id, author_login) = match value.get("user") {
        Some(Value::Null) => (None, None),
        Some(user) => {
            let identity = super::verify_identity(user, None)?;
            (Some(identity.id), Some(identity.login))
        }
        None => return Err(ConnectionError::InvalidResponse),
    };
    Ok(TopComment {
        id,
        body: text("body")?,
        author_id,
        author_login,
        created_at,
        updated_at,
    })
}

impl<T: Transport> GithubClient<T> {
    pub fn top_comments(
        &self,
        repo: &RemoteRepository,
        number: u64,
    ) -> Result<Vec<TopComment>, ConnectionError> {
        let name = crate::storage::canonical_repository(&repo.name)
            .map_err(|_| ConnectionError::InvalidRepository)?;
        if number == 0 {
            return Err(ConnectionError::InvalidResponse);
        }
        let raw = self.pages_limited(
            &format!("/repos/{name}/issues/{number}/comments?per_page=100&page=1"),
            1000,
        )?;
        let mut ids = HashSet::new();
        let mut bytes = 0;
        raw.iter()
            .map(|value| {
                if !value["issue_url"].as_str().is_some_and(|url| {
                    url.eq_ignore_ascii_case(&format!(
                        "https://api.github.com/repos/{name}/issues/{number}"
                    ))
                }) {
                    return Err(ConnectionError::RepositoryChanged);
                }
                let comment = parse(value)?;
                bytes += comment.body.len();
                if bytes > 1024 * 1024 || !ids.insert(comment.id.clone()) {
                    return Err(ConnectionError::IncompleteRead);
                }
                Ok(comment)
            })
            .collect()
    }
}

impl<T: MutationTransport> GithubClient<T> {
    pub fn reply_to_mention(
        &self,
        repo: &RemoteRepository,
        number: u64,
        account: &str,
        body: &str,
    ) -> Result<String, WriteFailure> {
        let name = crate::storage::canonical_repository(&repo.name).map_err(|e| WriteFailure {
            failure: Failure::permanent(e),
            uncertain: false,
        })?;
        if number == 0 || body.chars().count() > 65_536 {
            return Err(WriteFailure {
                failure: Failure::permanent("Invalid mention reply."),
                uncertain: false,
            });
        }

        let response = self
            .transport
            .mutate(
                &format!("/repos/{name}/issues/{number}/comments"),
                Request::Post(json!({"body":body})),
            )
            .map_err(|e| WriteFailure {
                failure: e.into(),
                uncertain: true,
            })?;
        if response.status != 201 {
            let uncertain = !(400..500).contains(&response.status) || response.status == 408;
            return Err(WriteFailure {
                failure: parse_response(response)
                    .err()
                    .map(Failure::from)
                    .unwrap_or_else(|| {
                        Failure::permanent("GitHub did not confirm the mention reply.")
                    }),
                uncertain,
            });
        }
        let parsed = (|| {
            let value: Value = serde_json::from_slice(&response.body)
                .map_err(|_| ConnectionError::InvalidResponse)?;
            let comment = parse(&value)?;
            if comment.author_id.as_deref() != Some(account)
                || comment.body != body
                || !value["issue_url"].as_str().is_some_and(|url| {
                    url.eq_ignore_ascii_case(&format!(
                        "https://api.github.com/repos/{name}/issues/{number}"
                    ))
                })
            {
                return Err(ConnectionError::InvalidResponse);
            }
            Ok(comment.id)
        })();
        parsed.map_err(|e: ConnectionError| WriteFailure {
            failure: e.into(),
            uncertain: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};
    struct Wire {
        values: Vec<Value>,
        calls: Arc<Mutex<usize>>,
    }
    impl Transport for Wire {
        fn get(&self, path: &str) -> Result<super::super::provider::Response, ConnectionError> {
            assert!(path.starts_with("/repos/example/repo/issues/1/comments?per_page=100&page="));
            *self.calls.lock().unwrap() += 1;
            let page = path
                .rsplit("page=")
                .next()
                .unwrap()
                .parse::<usize>()
                .unwrap();
            let start = (page - 1) * 100;
            let end = (start + 100).min(self.values.len());
            let mut headers = BTreeMap::new();
            if end < self.values.len() {
                headers.insert("link".into(),format!("<https://api.github.com/repos/example/repo/issues/1/comments?per_page=100&page={}>; rel=\"next\"",page+1));
            }
            Ok(super::super::provider::Response {
                status: 200,
                headers,
                body: serde_json::to_vec(&self.values[start..end]).unwrap(),
            })
        }
    }
    fn value(id: u64) -> Value {
        json!({"id":id,"body":"@actor a question","user":{"id":11,"login":"author"},
        "issue_url":"https://api.github.com/repos/example/repo/issues/1","created_at":"2026-09-30T00:00:00Z","updated_at":"2026-09-30T00:00:00Z"})
    }
    #[test]
    fn top_level_reads_are_complete_bounded_and_scoped_to_the_exact_pr() {
        let repo = RemoteRepository {
            id: "100".into(),
            name: "example/repo".into(),
        };
        let calls = Arc::new(Mutex::new(0));
        let client = GithubClient::new(Wire {
            values: (1..=101).map(value).collect(),
            calls: calls.clone(),
        });
        assert_eq!(client.top_comments(&repo, 1).unwrap().len(), 101);
        assert_eq!(*calls.lock().unwrap(), 2);
        for case in [
            "duplicate",
            "foreign",
            "invalid_time",
            "too_many",
            "too_large",
            "missing_user",
        ] {
            let mut values = vec![value(1)];
            match case {
                "duplicate" => values.push(value(1)),
                "foreign" => {
                    values[0]["issue_url"] = json!("https://github.com/foreign/repo/issues/1")
                }
                "invalid_time" => values[0]["updated_at"] = json!("unknown"),
                "too_many" => values = (1..=1001).map(value).collect(),
                "missing_user" => {
                    values[0].as_object_mut().unwrap().remove("user");
                }
                _ => values[0]["body"] = json!("x".repeat(1024 * 1024 + 1)),
            }
            let client = GithubClient::new(Wire {
                values,
                calls: Arc::new(Mutex::new(0)),
            });
            assert!(client.top_comments(&repo, 1).is_err(), "{case}");
        }
    }
}

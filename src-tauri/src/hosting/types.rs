use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Github,
    Gitlab,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Site {
    pub provider: Provider,
    pub host: String,
}
impl Site {
    pub fn validate(&self) -> Result<()> {
        let url = Url::parse(&format!("https://{}/", self.host))?;
        if self.host.len() > 255
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || url.authority() != self.host
            || self.host.contains(['/', '\\'])
        {
            bail!("Use a canonical host with an optional port, without a URL or credentials");
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewRef {
    pub site: Site,
    pub repository: String,
    pub number: u64,
}
impl ReviewRef {
    pub fn validate(&self) -> Result<()> {
        self.site.validate()?;
        let parts = self.repository.split('/').collect::<Vec<_>>();
        if self.number == 0
            || parts.len() < 2
            || self.site.provider == Provider::Github && parts.len() != 2
            || self.repository.len() > 1024
            || parts.iter().any(|s| {
                s.is_empty()
                    || *s == "."
                    || *s == ".."
                    || !s
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
            })
        {
            bail!("Invalid hosted review reference");
        }
        Ok(())
    }
    pub fn url(&self) -> String {
        format!(
            "https://{}/{}/{}{}/{}",
            self.site.host,
            self.repository,
            if self.site.provider == Provider::Github {
                ""
            } else {
                "-/"
            },
            if self.site.provider == Provider::Github {
                "pull"
            } else {
                "merge_requests"
            },
            self.number
        )
    }
    pub fn parse(input: &str, provider: Provider) -> Result<Self> {
        let u = Url::parse(input)?;
        if u.scheme() != "https" || !u.username().is_empty() || u.password().is_some() {
            bail!("Use an HTTPS review URL without credentials");
        }
        let marker = if provider == Provider::Github {
            "/pull/"
        } else {
            "/-/merge_requests/"
        };
        let (repo, n) = u
            .path()
            .trim_matches('/')
            .split_once(marker)
            .ok_or_else(|| anyhow::anyhow!("Invalid review URL"))?;
        let r = Self {
            site: Site {
                provider,
                host: u.authority().into(),
            },
            repository: repo.into(),
            number: n.split('/').next().unwrap_or("").parse()?,
        };
        r.validate()?;
        Ok(r)
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Read {
    Discover {
        scope: String,
        page: u32,
    },
    Details {
        review: ReviewRef,
    },
    Checks {
        review: ReviewRef,
        sha: String,
        page: u32,
    },
    Conversation {
        review: ReviewRef,
        page: u32,
    },
    Threads {
        review: ReviewRef,
        page: u32,
        cursor: Option<String>,
    },
    Jobs {
        review: ReviewRef,
        sha: String,
        run: u64,
        attempt: Option<u32>,
        page: u32,
    },
    Log {
        review: ReviewRef,
        sha: String,
        job: u64,
    },
    Reconcile {
        operation: String,
    },
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Action {
    Comment {
        body: String,
    },
    Reply {
        thread: String,
        comment: Option<String>,
        body: String,
    },
    Resolve {
        thread: String,
        resolved: bool,
    },
    Submit {
        verdict: String,
        body: String,
    },
    Merge {
        method: String,
    },
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Write {
    pub operation: String,
    pub review: ReviewRef,
    pub expected_head: String,
    pub action: Action,
}
pub fn sha(value: &str) -> Result<()> {
    if ![40, 64].contains(&value.len()) || !value.bytes().all(|c| c.is_ascii_hexdigit()) {
        bail!("Expected an exact commit SHA");
    }
    Ok(())
}
pub fn page(value: u32) -> Result<u32> {
    if !(1..=100).contains(&value) {
        bail!("Page is outside the supported range");
    }
    Ok(value)
}
pub fn identifier(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 200
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_=-".contains(&b))
    {
        bail!("Invalid provider identifier");
    }
    Ok(())
}
pub fn body(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 32 * 1024 || value.contains('\0') {
        bail!("Use a comment of 1–32768 bytes");
    }
    Ok(())
}
pub fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .chars()
        .take(32768)
        .collect()
}
pub fn link(value: &Value, key: &str) -> Option<String> {
    let value = value.get(key)?.as_str()?;
    let u = Url::parse(value).ok()?;
    (u.scheme() == "https" && u.username().is_empty() && u.password().is_none())
        .then(|| value.to_owned())
}
pub fn items(value: &Value) -> Vec<Value> {
    value.as_array().cloned().unwrap_or_default()
}

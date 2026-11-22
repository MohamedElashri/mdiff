//! Host-bound transport with bounded bodies, shared admission, conditional reads and cancellation.
use super::types::*;
use anyhow::{bail, Result};
use reqwest::{Client, Method, StatusCode};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime},
};
use tokio::sync::{watch, Mutex, Semaphore};
use url::Url;
const JSON_LIMIT: usize = 2 * 1024 * 1024;
const LOG_LIMIT: usize = 128 * 1024;
struct Cached {
    value: Value,
    etag: Option<String>,
    at: Instant,
    bytes: usize,
}
pub struct Broker {
    client: Client,
    slots: Semaphore,
    cache: Mutex<HashMap<String, Cached>>,
    flights: Mutex<HashMap<String, std::sync::Weak<Mutex<()>>>>,
    cooldown: Mutex<HashMap<String, Instant>>,
    epoch: watch::Sender<u64>,
    paused: AtomicBool,
}
pub struct Pause<'a>(&'a Broker);
impl Drop for Pause<'_> {
    fn drop(&mut self) {
        self.0.epoch.send_modify(|e| *e += 1);
        self.0.paused.store(false, Ordering::SeqCst);
    }
}
#[derive(Clone)]
pub struct Connection {
    pub site: Site,
    pub base: Url,
    pub token: Option<String>,
}
impl Connection {
    pub fn new(site: Site, token: Option<String>) -> Result<Self> {
        site.validate()?;
        let base = match site.provider {
            Provider::Github => diffle::github::hosts::GithubInstance {
                host: site.host.clone(),
                git_hosts: vec![],
            }
            .api_base_url(),
            Provider::Gitlab => format!("https://{}/api/v4", site.host),
        };
        Ok(Self {
            site,
            base: Url::parse(&base)?,
            token,
        })
    }
    pub fn url(&self, path: &str, args: &[(&str, String)]) -> Result<Url> {
        if !path.starts_with('/')
            || path.contains(['\\', '#', '\n', '\r'])
            || path.contains("://")
            || path.split('/').any(|s| s == "..")
        {
            bail!("Invalid provider endpoint");
        }
        let mut url = self.base.clone();
        if path == "/graphql" {
            url.set_path(&format!(
                "{}/graphql",
                self.base
                    .path()
                    .trim_end_matches("/v3")
                    .trim_end_matches('/')
            ));
        } else {
            url.set_path(&format!(
                "{}{}",
                self.base.path().trim_end_matches('/'),
                path
            ));
        }
        if !args.is_empty() {
            url.query_pairs_mut()
                .extend_pairs(args.iter().map(|(k, v)| (*k, v.as_str())));
        }
        Ok(url)
    }
    pub fn project(&self, review: &ReviewRef) -> String {
        let encoded =
            url::form_urlencoded::byte_serialize(review.repository.as_bytes()).collect::<String>();
        match self.site.provider {
            Provider::Github => format!("/repos/{}", review.repository),
            Provider::Gitlab => format!("/projects/{encoded}"),
        }
    }
    pub fn review_path(&self, review: &ReviewRef) -> String {
        format!(
            "{}/{}/{}",
            self.project(review),
            if self.site.provider == Provider::Github {
                "pulls"
            } else {
                "merge_requests"
            },
            review.number
        )
    }
}
impl Default for Broker {
    fn default() -> Self {
        Self {
            client: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(25))
                .build()
                .expect("HTTP client"),
            slots: Semaphore::new(2),
            cache: Mutex::default(),
            flights: Mutex::default(),
            cooldown: Mutex::default(),
            epoch: watch::channel(0).0,
            paused: AtomicBool::new(false),
        }
    }
}
impl Broker {
    #[cfg(test)]
    pub async fn cache_size(&self) -> (usize, usize) {
        let cache = self.cache.lock().await;
        (cache.len(), cache.values().map(|v| v.bytes).sum())
    }
    pub fn changes(&self) -> watch::Receiver<u64> {
        self.epoch.subscribe()
    }
    pub fn epoch(&self) -> u64 {
        *self.epoch.borrow()
    }
    pub fn pause(&self) -> Pause<'_> {
        self.paused.store(true, Ordering::SeqCst);
        self.epoch.send_modify(|e| *e += 1);
        Pause(self)
    }
    pub async fn clear(&self) {
        self.cache.lock().await.clear();
    }
    pub async fn read(
        &self,
        c: &Connection,
        method: Method,
        path: &str,
        args: &[(&str, String)],
        body: Option<Value>,
        fresh: bool,
    ) -> Result<Value> {
        if self.paused.load(Ordering::SeqCst) {
            bail!("Connection settings are changing; reload shortly");
        }
        let mut epoch = self.epoch.subscribe();
        let generation = *epoch.borrow();
        let key = format!(
            "{generation}:{:?}:{}:{}:{}:{}",
            c.site.provider,
            c.site.host,
            crate::storage::digest(c.token.as_deref().unwrap_or("anonymous")),
            c.url(path, args)?,
            body.as_ref().map(Value::to_string).unwrap_or_default()
        );
        let lock = {
            let mut flights = self.flights.lock().await;
            flights.retain(|_, v| v.strong_count() > 0);
            let lock = flights
                .get(&key)
                .and_then(std::sync::Weak::upgrade)
                .unwrap_or_else(|| Arc::new(Mutex::new(())));
            flights.insert(key.clone(), Arc::downgrade(&lock));
            lock
        };
        let work = async {
            let _flight = lock.lock().await;
            let (old, etag) = self
                .cache
                .lock()
                .await
                .get(&key)
                .map(|v| {
                    (
                        (!fresh && v.at.elapsed() < Duration::from_secs(20))
                            .then(|| v.value.clone()),
                        v.etag.clone(),
                    )
                })
                .unwrap_or_default();
            if let Some(value) = old {
                return Ok(value);
            }
            let _slot = self.slots.acquire().await?;
            self.backoff(c).await?;
            let response = self
                .send(c, method, path, args, body, etag.as_deref())
                .await?;
            let status = response.status();
            self.rate(c, &response).await;
            let etag = response
                .headers()
                .get("etag")
                .and_then(|h| h.to_str().ok())
                .map(str::to_owned)
                .or(etag);
            let value = if status == StatusCode::NOT_MODIFIED {
                self.cache
                    .lock()
                    .await
                    .get(&key)
                    .map(|v| v.value.clone())
                    .ok_or_else(|| anyhow::anyhow!("Conditional response has no cached value"))?
            } else {
                check(status)?;
                let bytes = bounded(response, JSON_LIMIT).await?;
                if bytes.is_empty() {
                    Value::Null
                } else {
                    serde_json::from_slice(&bytes)?
                }
            };
            if self.epoch() != generation {
                bail!("Connection changed; response discarded");
            }
            let mut cache = self.cache.lock().await;
            cache.retain(|k, _| k.starts_with(&format!("{generation}:")));
            let bytes = serde_json::to_vec(&value)?.len();
            cache.remove(&key);
            while cache.len() >= 32
                || cache.values().map(|v| v.bytes).sum::<usize>() + bytes > 8 * 1024 * 1024
            {
                if let Some(oldest) = cache
                    .iter()
                    .min_by_key(|(_, v)| v.at)
                    .map(|(k, _)| k.clone())
                {
                    cache.remove(&oldest);
                } else {
                    break;
                }
            }
            cache.insert(
                key,
                Cached {
                    value: value.clone(),
                    etag,
                    at: Instant::now(),
                    bytes,
                },
            );
            Ok(value)
        };
        tokio::select! {result=work=>result,_=epoch.changed()=>{bail!("Connection changed; request cancelled")}}
    }
    pub async fn write(
        &self,
        c: &Connection,
        method: Method,
        path: &str,
        body: Value,
    ) -> Result<Value> {
        let _slot = self.slots.acquire().await?;
        self.backoff(c).await?;
        let response = self.send(c, method, path, &[], Some(body), None).await?;
        self.rate(c, &response).await;
        check(response.status())?;
        let bytes = bounded(response, JSON_LIMIT).await?;
        Ok(if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)?
        })
    }
    async fn send(
        &self,
        c: &Connection,
        method: Method,
        path: &str,
        args: &[(&str, String)],
        body: Option<Value>,
        etag: Option<&str>,
    ) -> Result<reqwest::Response> {
        let mut request = self
            .client
            .request(method, c.url(path, args)?)
            .header("User-Agent", "mdiff");
        match c.site.provider {
            Provider::Github => {
                request = request.header("Accept", "application/vnd.github+json");
                if let Some(token) = &c.token {
                    request = request.bearer_auth(token);
                }
            }
            Provider::Gitlab => {
                if let Some(token) = &c.token {
                    request = request.header("PRIVATE-TOKEN", token);
                }
            }
        }
        if let Some(etag) = etag {
            request = request.header("If-None-Match", etag);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        request.send().await.map_err(|_| {
            anyhow::anyhow!("Cannot reach the hosting API; the outcome of a write may be unknown")
        })
    }
    async fn backoff(&self, c: &Connection) -> Result<()> {
        let cooldown = self.cooldown.lock().await;
        if [c.site.host.as_str(), "*"].iter().any(|host| {
            cooldown
                .get(*host)
                .is_some_and(|until| *until > Instant::now())
        }) {
            bail!("Provider rate limit: wait before refreshing or writing again");
        }
        Ok(())
    }
    async fn rate(&self, c: &Connection, response: &reqwest::Response) {
        let headers = response.headers();
        let after = headers
            .get("retry-after")
            .and_then(|h| h.to_str().ok())
            .and_then(|h| {
                h.parse::<u64>().ok().map(Duration::from_secs).or_else(|| {
                    httpdate::parse_http_date(h)
                        .ok()?
                        .duration_since(SystemTime::now())
                        .ok()
                })
            });
        let exhausted = headers
            .get("x-ratelimit-remaining")
            .is_some_and(|v| v == "0");
        let reset = headers
            .get("x-ratelimit-reset")
            .and_then(|h| h.to_str().ok()?.parse::<u64>().ok())
            .and_then(|s| {
                SystemTime::UNIX_EPOCH
                    .checked_add(Duration::from_secs(s))?
                    .duration_since(SystemTime::now())
                    .ok()
            });
        if response.status() == StatusCode::TOO_MANY_REQUESTS || exhausted || after.is_some() {
            let duration = after.or(reset).unwrap_or(Duration::from_secs(60));
            if let Some(until) = Instant::now().checked_add(duration) {
                let mut cooldown = self.cooldown.lock().await;
                if cooldown.len() >= 32 {
                    cooldown.retain(|_, v| *v > Instant::now());
                }
                if cooldown.len() >= 32 {
                    let until = cooldown.values().copied().max().unwrap_or(until).max(until);
                    cooldown.clear();
                    cooldown.insert("*".into(), until);
                } else {
                    cooldown.insert(c.site.host.clone(), until);
                }
            }
        }
    }
    pub async fn log(&self, c: &Connection, path: &str) -> Result<Value> {
        if self.paused.load(Ordering::SeqCst) {
            bail!("Connection settings are changing; reload shortly");
        }
        let mut epoch = self.epoch.subscribe();
        let work = async {
            let _slot = self.slots.acquire().await?;
            self.backoff(c).await?;
            let response = self.send(c, Method::GET, path, &[], None, None).await?;
            self.rate(c, &response).await;
            let response = if response.status().is_redirection()
                && c.site.provider == Provider::Github
            {
                let url = response
                    .headers()
                    .get("location")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| Url::parse(v).ok())
                    .ok_or_else(|| anyhow::anyhow!("Log download URL is unavailable"))?;
                if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some()
                {
                    bail!("Unsafe log download URL");
                }
                // Signed storage URLs receive no provider authorization header, and are never cached/logged.
                self.client
                    .get(url)
                    .header("Range", format!("bytes=0-{}", LOG_LIMIT - 1))
                    .send()
                    .await
                    .map_err(|_| anyhow::anyhow!("Cannot read the CI log download"))?
            } else {
                response
            };
            check(response.status())?;
            let (bytes, truncated) = excerpt(response, LOG_LIMIT).await?;
            let truncated = truncated || bytes.len() == LOG_LIMIT;
            let text = String::from_utf8_lossy(&bytes)
                .chars()
                .filter(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
                .collect::<String>();
            Ok(json!({"text":text,"truncated":truncated,"limit":LOG_LIMIT}))
        };
        tokio::select! {result=work=>result,_=epoch.changed()=>{bail!("Connection changed; log request cancelled")}}
    }
}
#[derive(Debug)]
pub struct Rejection(pub u16);
impl std::fmt::Display for Rejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Hosting API rejected this request (HTTP {}). Check permissions and revision.",
            self.0
        )
    }
}
impl std::error::Error for Rejection {}
fn check(status: StatusCode) -> Result<()> {
    if !status.is_success() {
        if status.is_client_error() {
            return Err(Rejection(status.as_u16()).into());
        }
        bail!(
            "Hosting API returned {}; reconcile writes before retrying",
            status.as_u16()
        );
    }
    Ok(())
}
async fn excerpt(mut response: reqwest::Response, limit: usize) -> Result<(Vec<u8>, bool)> {
    let mut bytes = vec![];
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("Provider response was interrupted"))?
    {
        let remaining = limit - bytes.len();
        if chunk.len() > remaining {
            bytes.extend_from_slice(&chunk[..remaining]);
            return Ok((bytes, true));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok((bytes, false))
}
async fn bounded(response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    let (bytes, truncated) = excerpt(response, limit).await?;
    if truncated {
        bail!("Provider response exceeded its payload limit");
    }
    Ok(bytes)
}

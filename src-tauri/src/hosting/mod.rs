//! Desktop-only hosting context. Shared review data and local feedback keep their own authority.
pub(crate) mod attention;
mod http;
mod ledger;
mod mutations;
mod provider;
pub(crate) mod types;
use anyhow::{bail, Result};
use http::Broker;
pub(crate) use http::Connection;
use ledger::Ledger;
use serde_json::{json, Value};
use tauri::{Manager, State, WebviewWindow};
use types::*;
#[derive(Default)]
pub struct Hosting {
    pub broker: Broker,
    pub(crate) reads: crate::transport::Requests,
}
fn ledger(app: &tauri::AppHandle) -> Result<Ledger> {
    Ok(Ledger {
        file: app
            .path()
            .app_data_dir()?
            .join("hosting-operations-v1.json"),
    })
}
fn reference(source: &crate::workspaces::Source, fallback: Option<String>) -> Result<ReviewRef> {
    if source.request["kind"] == "gitlab" {
        return ReviewRef::parse(
            source.request["url"].as_str().unwrap_or(""),
            Provider::Gitlab,
        );
    }
    let url = fallback
        .or_else(|| source.request["pr"].as_str().map(str::to_owned))
        .ok_or_else(|| {
            anyhow::anyhow!("Open a pull request or merge request from Pull & merge requests first")
        })?;
    ReviewRef::parse(&url, Provider::Github)
}
pub(crate) async fn connection(app: &tauri::AppHandle, site: Site) -> Result<Connection> {
    site.validate()?;
    let engine = app.state::<crate::engine::Engine>();
    let config = engine
        .config(app.path().app_config_dir()?.join("review.json"))
        .await;
    let credentials = app
        .state::<crate::settings::MemorySettings>()
        .credentials()
        .await;
    let token = match site.provider {
        Provider::Github => {
            if !crate::github::Connections::load(config.file())
                .await?
                .github_enabled
            {
                bail!("GitHub integration is disabled in Connections");
            }
            let explicit = if credentials.token_host.as_deref() == Some(&site.host) {
                credentials.token.filter(|s| !s.is_empty())
            } else {
                None
            };
            if explicit.is_some() {
                explicit
            } else {
                match tokio::time::timeout(
                    std::time::Duration::from_secs(6),
                    diffle::github::client::resolve_token(&site.host, |key| {
                        std::env::var(key).ok()
                    }),
                )
                .await
                {
                    Ok(v) => v.map(|(token, _)| token),
                    Err(_) => None,
                }
            }
        }
        Provider::Gitlab => {
            if credentials.gitlab_host.as_deref() == Some(&site.host) {
                credentials.gitlab_token.filter(|s| !s.is_empty())
            } else {
                None
            }
        }
    };
    Connection::new(site, token)
}
async fn current(
    app: &tauri::AppHandle,
    window: &WebviewWindow,
    id: &str,
) -> Result<(ReviewRef, String, String)> {
    let source = app
        .state::<crate::workspaces::Sessions>()
        .hosting_source(app, window.label(), id)
        .await?;
    let engine = app.state::<crate::engine::Engine>();
    let runtime = engine
        .owned_runtime(id, window.label())
        .await
        .map_err(anyhow::Error::msg)?;
    let r = reference(&source, runtime.hosting_pr()?)?;
    let (old, head) = runtime.hosting_revision().await?;
    sha(&old)?;
    sha(&head)?;
    Ok((r, old, head))
}
#[tauri::command]
pub async fn hosting_context(
    app: tauri::AppHandle,
    window: WebviewWindow,
    id: Option<String>,
) -> Result<Value, String> {
    crate::windows::authorize(&window)?;
    let result = async {
        let generation = app.state::<Hosting>().broker.epoch();
        let engine = app.state::<crate::engine::Engine>();
        let config = engine
            .config(app.path().app_config_dir()?.join("review.json"))
            .await;
        let credentials = app
            .state::<crate::settings::MemorySettings>()
            .credentials()
            .await;
        let enabled = crate::github::Connections::load(config.file())
            .await?
            .github_enabled;
        let mut sites = vec![];
        if enabled {
            sites.push(Site {
                provider: Provider::Github,
                host: "github.com".into(),
            });
            for host in config.get().github_hosts.values() {
                let site = Site {
                    provider: Provider::Github,
                    host: host.clone(),
                };
                if site.validate().is_ok() {
                    sites.push(site);
                }
            }
            if let Some(host) = credentials.token_host {
                sites.push(Site {
                    provider: Provider::Github,
                    host,
                });
            }
        }
        sites.push(Site {
            provider: Provider::Gitlab,
            host: credentials
                .gitlab_host
                .unwrap_or_else(|| "gitlab.com".into()),
        });
        let selected = match id {
            Some(id) => current(&app, &window, &id)
                .await
                .ok()
                .map(|(r, base, sha)| json!({"review":r,"base":base,"sha":sha})),
            None => None,
        };
        if let Some(value) = &selected {
            let site: Site = serde_json::from_value(value["review"]["site"].clone())?;
            if site.provider != Provider::Github || enabled {
                sites.push(site);
            }
        }
        anyhow::ensure!(
            generation == app.state::<Hosting>().broker.epoch(),
            "Connection settings changed; reload the pull or merge request"
        );
        sites.sort_by_key(|s| format!("{:?}:{}", s.provider, s.host));
        sites.dedup();
        Ok::<_, anyhow::Error>(
            json!({"sites":sites,"current":selected,"epoch":app.state::<Hosting>().broker.epoch()}),
        )
    }
    .await;
    result.map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn hosting_read(
    app: tauri::AppHandle,
    window: WebviewWindow,
    state: State<'_, Hosting>,
    site: Site,
    request: Read,
    request_id: String,
) -> Result<Value, String> {
    crate::windows::authorize(&window)?;
    identifier(&request_id).map_err(|e| e.to_string())?;
    let (_request, cancelled) = state.reads.start((window.label().into(), request_id))?;
    let mut changes = state.broker.changes();
    let work = async {
        let sessions = app.state::<crate::workspaces::Sessions>();
        let _work = sessions.gate.read().await;
        app.state::<crate::windows::Windows>()
            .admit(window.label())
            .map_err(anyhow::Error::msg)?;
        // The frontend may select an explicit host, but credentials are resolved by native exact-host binding.
        site.validate()?;
        let c = connection(&app, site.clone()).await?;
        let review = match &request {
            Read::Details { review }
            | Read::Checks { review, .. }
            | Read::Conversation { review, .. }
            | Read::Threads { review, .. }
            | Read::Jobs { review, .. }
            | Read::Log { review, .. } => Some(review),
            _ => None,
        };
        if let Some(r) = review {
            r.validate()?;
            anyhow::ensure!(r.site == site, "Review and connection hosts differ");
        }
        match request {
            Read::Discover { scope, page } => state.broker.discover(&c, &scope, page).await,
            Read::Details { review } => state.broker.details(&c, &review, false).await,
            Read::Checks { review, sha, page } => {
                state.broker.checks(&c, &review, &sha, page).await
            }
            Read::Conversation { review, page } => {
                state.broker.conversation(&c, &review, page, false).await
            }
            Read::Threads {
                review,
                page,
                cursor,
            } => {
                state
                    .broker
                    .threads(&c, &review, page, cursor.as_deref())
                    .await
            }
            Read::Jobs {
                review,
                sha,
                run,
                attempt,
                page,
            } => {
                state
                    .broker
                    .jobs(&c, &review, &sha, run, attempt, page)
                    .await
            }
            Read::Log { review, sha, job } => state.broker.job_log(&c, &review, &sha, job).await,
            Read::Reconcile { operation } => {
                identifier(&operation)?;
                let ledger = ledger(&app)?;
                let record = ledger.find(&operation).await?;
                anyhow::ensure!(
                    record.review.site == site,
                    "Operation belongs to another host"
                );
                let actor = state.broker.user(&c).await?["id"].to_string();
                anyhow::ensure!(
                    actor == record.actor,
                    "Reconnect the account that submitted this operation"
                );
                if matches!(record.receipt.status.as_str(), "succeeded" | "rejected") {
                    return Ok(serde_json::to_value(record.receipt)?);
                }
                let proof = state.broker.reconcile(&c, &record).await?;
                Ok(serde_json::to_value(if let Some(proof) = proof {
                    ledger
                        .finish(
                            record,
                            "succeeded",
                            "Provider state confirms the operation or its intended result.",
                            Some(&proof),
                        )
                        .await?
                } else {
                    ledger.finish(record,"unknown","Not confirmed within the bounded reconciliation window. Inspect the provider page before taking further action.",None).await?
                })?)
            }
        }
    };
    tokio::select! {result=work=>result.map_err(|e|e.to_string()),_=changes.changed()=>Err("Connection changed; provider request cancelled".into()),_=cancelled=>Err("Provider request cancelled".into())}
}
#[tauri::command]
pub async fn hosting_cancel(
    window: WebviewWindow,
    state: State<'_, Hosting>,
    request_id: String,
) -> Result<(), String> {
    crate::windows::authorize(&window)?;
    identifier(&request_id).map_err(|e| e.to_string())?;
    state.reads.cancel((window.label().into(), request_id));
    Ok(())
}
#[tauri::command]
pub async fn hosting_write(
    app: tauri::AppHandle,
    window: WebviewWindow,
    id: String,
    request: Write,
) -> Result<ledger::Receipt, String> {
    crate::windows::authorize(&window)?;
    // Accepted writes finish and persist a receipt even if their WebView drops the invoke promise.
    tauri::async_runtime::spawn(async move {
        let mut prepared = false;
        match write(&app, &window, &id, &request, &mut prepared).await {
            Ok(receipt) => Ok(receipt),
            Err(error) => Ok(ledger::Receipt {
                operation: request.operation.clone(),
                status: if prepared { "unknown" } else { "rejected" }.into(),
                reviewed_sha: request.expected_head.clone(),
                url: request.review.url(),
                message: error.to_string(),
                remote_id: None,
                result_sha: None,
            }),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
async fn write(
    app: &tauri::AppHandle,
    window: &WebviewWindow,
    id: &str,
    request: &Write,
    prepared: &mut bool,
) -> Result<ledger::Receipt> {
    request.review.validate()?;
    sha(&request.expected_head)?;
    mutations::validate(&request.action)?;
    let sessions = app.state::<crate::workspaces::Sessions>();
    let _work = sessions.gate.read().await;
    app.state::<crate::windows::Windows>()
        .admit(window.label())
        .map_err(anyhow::Error::msg)?;
    let (reference, old, head) = current(app, window, id).await?;
    anyhow::ensure!(
        reference == request.review && head == request.expected_head,
        "Review target or recorded revision changed. Reopen and inspect it before submitting."
    );
    let engine = app.state::<crate::engine::Engine>();
    let runtime = engine
        .owned_runtime(id, window.label())
        .await
        .map_err(anyhow::Error::msg)?;
    // Recheck under the guard kept through provider I/O and durable receipt publication.
    anyhow::ensure!(
        runtime.hosting_revision().await? == (old.clone(), head.clone()),
        "Review revision changed"
    );
    let state = app.state::<Hosting>();
    let c = connection(app, request.review.site.clone()).await?;
    anyhow::ensure!(c.token.is_some(), "Connect this host before writing");
    let actor = state.broker.user(&c).await?["id"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("Cannot identify provider account"))?
        .to_string();
    let ledger = ledger(app)?;
    // Retry of a prepared operation returns its receipt before any remote mutation.
    if ledger.find(&request.operation).await.is_ok() {
        let (record, _) = ledger.begin(request, &actor).await?;
        return Ok(record.receipt);
    }
    let details = state.broker.details(&c, &reference, true).await?;
    let base = details["base"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Provider comparison base is unavailable"))?;
    sha(base)?;
    let actual_base = if reference.site.provider == Provider::Github {
        runtime.hosting_base(base, &head).await?
    } else {
        base.into()
    };
    validate_provider(request, &old, &head, &actual_base, &details)?;
    dispatch(&state.broker, &c, &ledger, request, &actor, prepared).await
}
fn validate_provider(
    request: &Write,
    old: &str,
    head: &str,
    actual_base: &str,
    details: &Value,
) -> Result<()> {
    anyhow::ensure!(
        request.expected_head == head && details["head"] == head,
        "The provider head changed. Refresh and review the new revision before submitting."
    );
    anyhow::ensure!(
        details["state"] == "open" || details["state"] == "opened",
        "This review is no longer open"
    );
    anyhow::ensure!(old==actual_base,"The opened comparison does not match the provider diff base. Refresh the complete review before submitting.");
    if let Action::Merge { method } = &request.action {
        anyhow::ensure!(
            details["capabilities"]["mergeMethods"]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v == method)),
            "This merge method is unavailable for your permissions or project policy"
        );
        anyhow::ensure!(
            details["draft"] != true,
            "Publish the draft review before merging"
        );
    }
    Ok(())
}
async fn dispatch(
    broker: &Broker,
    c: &Connection,
    ledger: &Ledger,
    request: &Write,
    actor: &str,
    prepared: &mut bool,
) -> Result<ledger::Receipt> {
    broker.prepare_action(c, request).await?;
    let (record, new) = ledger.begin(request, actor).await?;
    *prepared = true;
    if !new {
        return Ok(record.receipt);
    }
    match broker.mutate(c, request).await {
        Ok(value) => {
            let merge = matches!(request.action, Action::Merge { .. });
            let confirmed = if merge {
                if c.site.provider == Provider::Github {
                    value["merged"] == true
                } else {
                    value["state"] == "merged"
                }
            } else {
                true
            };
            let status = if confirmed { "succeeded" } else { "unknown" };
            ledger.finish(record,status,if confirmed{if c.site.provider==Provider::Github&&matches!(request.action,Action::Submit{..}){"GitHub accepted the review pinned to the recorded commit_id. The final head check is not atomic."}else{"Provider accepted the action for the recorded reviewed revision."}}else{"Merge is queued, pending, or unconfirmed. Reconcile its state; do not resubmit."},Some(&value)).await
        }
        Err(error) => {
            let status = if error.downcast_ref::<http::Rejection>().is_some() {
                "rejected"
            } else {
                "unknown"
            };
            ledger
                .finish(record, status, &error.to_string(), None)
                .await
        }
    }
}
#[cfg(test)]
mod tests;

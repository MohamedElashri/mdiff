//! Desktop-owned local Git jobs. No arbitrary argv, renderer paths for existing
//! repositories, read-request cancellation, or standalone mutation routes.
pub mod drafts;
mod ledger;
mod locks;
mod new_repository;
use anyhow::{Context, Result};
use diffle::{
    git::operations::{self, Action, Plan, Snapshot},
    persist::{with_file_lock, write_file_atomic},
};
use ledger::{Ledger, Receipt};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager, WebviewWindow};
use tokio::sync::{watch, Semaphore};
struct Preview {
    owner: String,
    context: Option<String>,
    created: Instant,
    plan: Prepared,
    bytes: usize,
}
enum Prepared {
    Repository(Box<Plan>),
    New(new_repository::Prepared),
}
struct Live {
    owner: String,
    cancel: watch::Sender<bool>,
}
pub struct Operations {
    previews: Mutex<HashMap<String, Preview>>,
    live: Mutex<HashMap<String, Live>>,
    pub locks: locks::Locks,
    jobs: Arc<Semaphore>,
    local: Arc<Semaphore>,
    network: Arc<Semaphore>,
    reads: Semaphore,
    pub(crate) requests: crate::transport::Requests,
}
impl Default for Operations {
    fn default() -> Self {
        Self {
            previews: Mutex::default(),
            live: Mutex::default(),
            locks: Default::default(),
            jobs: Arc::new(Semaphore::new(8)),
            local: Arc::new(Semaphore::new(2)),
            network: Arc::new(Semaphore::new(1)),
            reads: Semaphore::new(2),
            requests: Default::default(),
        }
    }
}
fn ledger(app: &tauri::AppHandle) -> Result<Ledger> {
    Ok(Ledger(
        app.path().app_data_dir()?.join("git-operations-v1.json"),
    ))
}
async fn target(
    app: &tauri::AppHandle,
    owner: &str,
    context: &str,
) -> Result<crate::worktree_tools::Target> {
    let target = crate::worktree_tools::tool_target(app, owner, context, None).await?;
    anyhow::ensure!(!target.managed() && !crate::worktree_tools::protected(app,&target.root),"App-managed PR/MR checkouts are read-only in Git operations. Use a user-owned clone instead.");
    Ok(target)
}
async fn in_use(app: &tauri::AppHandle, path: &Path) -> Result<bool> {
    if app
        .state::<crate::terminal::Terminals>()
        .uses_checkout(path)
    {
        return Ok(true);
    }
    if crate::worktree_tools::protected(app, path) {
        return Ok(true);
    }
    if app
        .state::<crate::engine::Engine>()
        .uses_checkout(path)
        .await
        || app
            .state::<crate::repository::Repositories>()
            .uses_checkout(path)
            .await
    {
        return Ok(true);
    }
    if crate::workspaces::worktree_associations(app)
        .await?
        .iter()
        .any(|entry| Path::new(&entry.path) == path)
    {
        return Ok(true);
    }
    let leases = external_file(app)?;
    let list: Vec<String> = if leases.exists() {
        anyhow::ensure!(
            tokio::fs::metadata(&leases).await?.len() <= 128 * 1024,
            "External-tool lease file exceeds its limit"
        );
        serde_json::from_slice(&tokio::fs::read(leases).await?)?
    } else {
        vec![]
    };
    Ok(list.contains(&crate::storage::digest(&format!("{:?}", path.as_os_str()))))
}
async fn guard_worktrees(app: &tauri::AppHandle, plan: &Plan) -> Result<()> {
    match &plan.action {
        Action::WorktreeRemove{path}=>anyhow::ensure!(!in_use(app,&tokio::fs::canonicalize(path).await?).await?,"Worktree is used by a review, History, embedded terminal, external tool, or managed checkout. Close its tabs/tools; externally opened folders remain protected across restarts and require external removal."),
        Action::WorktreePrune=>for row in diffle::git::worktrees::list(&plan.before.identity.root).await? {if row.prunable.is_some(){anyhow::ensure!(!in_use(app,&row.path).await?,"A missing worktree is still associated with a review/tool or managed checkout; prune externally after recovery");}},
        Action::WorktreeCreate{path,..}|Action::WorktreeUnlock{path}|Action::WorktreeLock{path,..}=>anyhow::ensure!(!crate::worktree_tools::protected(app,Path::new(path)),"Managed checkout metadata belongs to its owning review"),
        _=>()
    }
    Ok(())
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Read {
    Catalog,
    Upstream,
    Recovery,
    Conflict { path: String },
    RebaseCommits { upstream: String },
    Patch { path: String, staged: bool },
}
#[tauri::command]
pub async fn git_operation_read(
    app: tauri::AppHandle,
    window: WebviewWindow,
    request_id: String,
    context: String,
    query: Read,
) -> Result<Value, String> {
    crate::windows::authorize(&window)?;
    if request_id.len() > 100 {
        return Err("Invalid request ID".into());
    }
    let requests = app.state::<Operations>();
    let (_request, cancelled) = requests
        .requests
        .start((window.label().into(), request_id))?;
    let result = async {
        let sessions = app.state::<crate::workspaces::Sessions>();
        let _gate = sessions.gate.read().await;
        app.state::<crate::windows::Windows>()
            .admit(window.label())
            .map_err(anyhow::Error::msg)?;
        let operations = app.state::<Operations>();
        let _read = operations.reads.acquire().await?;
        let target = target(&app, window.label(), &context).await?;
        match query {
            Read::Catalog => operations::catalog(&target.root).await,
            Read::Upstream => operations::upstream(&target.root).await,
            Read::Recovery => Ok(serde_json::to_value(
                operations::advanced::recovery(&target.root).await?,
            )?),
            Read::Conflict { path } => Ok(serde_json::to_value(
                operations::advanced::conflict(&target.root, &path).await?,
            )?),
            Read::RebaseCommits { upstream } => Ok(serde_json::to_value(
                operations::advanced::rebase_commits(&target.root, &upstream).await?,
            )?),
            Read::Patch { path, staged } => Ok(serde_json::to_value(
                operations::patch(&target.root, &path, staged).await?,
            )?),
        }
    };
    let result = tokio::select! {value=tokio::time::timeout(Duration::from_secs(30),result)=>value.map_err(|_|"Git preview timed out".to_string())?,_=cancelled=>Err(anyhow::anyhow!("Git read cancelled"))};
    result.map_err(|e: anyhow::Error| e.to_string())
}
#[tauri::command]
pub async fn git_operation_preview(
    app: tauri::AppHandle,
    window: WebviewWindow,
    request_id: String,
    context: Option<String>,
    action: Action,
) -> Result<Value, String> {
    crate::windows::authorize(&window)?;
    if request_id.len() > 100 {
        return Err("Invalid request ID".into());
    }
    let requests = app.state::<Operations>();
    let (_request, cancelled) = requests
        .requests
        .start((window.label().into(), request_id))?;
    let result = async {
        let sessions = app.state::<crate::workspaces::Sessions>();
        let _gate = sessions.gate.read().await;
        app.state::<crate::windows::Windows>()
            .admit(window.label())
            .map_err(anyhow::Error::msg)?;
        let state = app.state::<Operations>();
        let _read = state.reads.acquire().await?;
        let plan = match action {
            Action::Init { .. } | Action::Clone { .. } => {
                Prepared::New(new_repository::prepare(action, &app).await?)
            }
            _ => {
                let context = context.as_deref().context("Choose a repository first")?;
                let target = target(&app, window.label(), context).await?;
                let plan = operations::prepare(&target.root, action).await?;
                guard_worktrees(&app, &plan).await?;
                Prepared::Repository(Box::new(plan))
            }
        };
        let id = crate::workspaces::id();
        let value = match &plan {
            Prepared::Repository(plan) => {
                json!({"id":id,"root":plan.before.identity.root,"label":plan.action.label(),"text":plan.preview,"before":plan.before,"patch":plan.patch})
            }
            Prepared::New(plan) => {
                json!({"id":id,"root":plan.path,"label":plan.action.label(),"text":plan.text})
            }
        };
        let bytes = serde_json::to_vec(&value)?.len()
            + match &plan {
                Prepared::Repository(p) => p
                    .steps
                    .iter()
                    .filter_map(|s| s.input.as_ref())
                    .map(Vec::len)
                    .sum::<usize>(),
                Prepared::New(_) => 0,
            };
        let mut previews = state.previews.lock().unwrap();
        previews.retain(|_, p| p.created.elapsed() < Duration::from_secs(300));
        anyhow::ensure!(
            previews.len() < 32
                && previews.values().map(|p| p.bytes).sum::<usize>() + bytes <= 8 * 1024 * 1024,
            "Close old Git previews or wait five minutes before preparing more"
        );
        previews.insert(
            id,
            Preview {
                owner: window.label().into(),
                context,
                created: Instant::now(),
                plan,
                bytes,
            },
        );
        Ok(value)
    };
    let result = tokio::select! {value=tokio::time::timeout(Duration::from_secs(30),result)=>value.map_err(|_|"Git preview timed out".to_string())?,_=cancelled=>Err(anyhow::anyhow!("Git read cancelled"))};
    result.map_err(|e: anyhow::Error| e.to_string())
}
#[tauri::command]
pub async fn git_operation_execute(
    app: tauri::AppHandle,
    window: WebviewWindow,
    preview: String,
    context: Option<String>,
) -> Result<Receipt, String> {
    crate::windows::authorize(&window)?;
    let result=async {
        let lifecycle=app.state::<crate::workspaces::Sessions>().gate.clone().read_owned().await;
        app.state::<crate::windows::Windows>().admit(window.label()).map_err(anyhow::Error::msg)?;
        let state=app.state::<Operations>();let slot=state.jobs.clone().try_acquire_owned().context("Git jobs are already active; wait before starting another")?;
        if let Some(record)=ledger(&app)?.list().await?.into_iter().find(|record|record.operation==preview){return Ok(record);}
        let ticket=state.take_preview(window.label(),context.as_deref(),&preview)?;
        let network=match &ticket.plan{Prepared::Repository(p)=>p.action.network(),Prepared::New(p)=>p.action.network()};
        let limit=if network{state.network.clone()}else{state.local.clone()};
        let admission=limit.try_acquire_owned().context("Git operation capacity is busy (two local jobs, one network job). Wait for an existing job to finish.")?;
        if let Prepared::Repository(plan)=&ticket.plan {
            let target=target(&app,window.label(),ticket.context.as_deref().context("Missing context")?).await?;
            anyhow::ensure!(target.root==plan.before.identity.root,"Repository context changed");
        }
        let receipt=match &ticket.plan {
            Prepared::Repository(plan)=>Receipt{operation:preview.clone(),target_root:plan.destination.as_ref().map(|path|path.to_string_lossy().into_owned()),intent:intent(&plan.action)?,root:plan.before.identity.root.to_str().context("Git writes require UTF-8 checkout paths")?.into(),label:plan.action.label().into(),status:"pending".into(),message:"Accepted by native job. Closing the dialog does not cancel it.".into(),before:Some(plan.before.clone()),after:None,remote:plan.remote.clone()},
            Prepared::New(plan)=>Receipt{operation:preview.clone(),target_root:None,intent:intent(&plan.action)?,root:plan.path.to_str().context("Destination requires UTF-8")?.into(),label:plan.action.label().into(),status:"pending".into(),message:"Destination reserved by a native-owned job; partial output will be kept.".into(),before:None,after:None,remote:None}
        };
        ledger(&app)?.put(receipt.clone()).await?;
        let (cancel,receiver)=watch::channel(false);state.live.lock().unwrap().insert(preview.clone(),Live{owner:window.label().into(),cancel});
        let app=app.clone();let mut current=receipt.clone();
        tauri::async_runtime::spawn(async move {
            let _lifecycle=lifecycle;let _job=slot;
            struct Cleanup{app:tauri::AppHandle,id:String}
            impl Drop for Cleanup {fn drop(&mut self){self.app.state::<Operations>().live.lock().unwrap().remove(&self.id);let _=self.app.emit("git-operations-changed",());}}
            let _cleanup=Cleanup{app:app.clone(),id:current.operation.clone()};
            let state=app.state::<Operations>();
            let run=async {
                let _admission=admission;
                anyhow::ensure!(!*receiver.borrow(),"Cancelled before Git started");
                match ticket.plan {
                    Prepared::Repository(plan)=>{
                        let held=state.locks.acquire(&plan.before.identity,plan.action.index_only());
                        let mut stop=receiver.clone();let _held=tokio::select!{result=held=>result?,_=stop.wait_for(|value|*value)=>anyhow::bail!("Cancelled while waiting for repository admission")};
                        guard_worktrees(&app,&plan).await?;
                        operations::execute(*plan,receiver).await
                    }
                    Prepared::New(plan)=>new_repository::execute(plan,receiver).await,
                }
            }.await;
            match run {Ok(outcome)=>current.finish(outcome),Err(error)=>{current.status="needsInspection".into();current.message=error.to_string().chars().take(2048).collect();}}
            let common=current.before.as_ref().map(|before|before.identity.common.clone());
            if let Ok(ledger)=ledger(&app){let _=ledger.put(current).await;}
            if let Some(common)=common {app.state::<crate::engine::Engine>().refresh_git(&common).await;}
            let _=app.emit("git-repository-changed",());
        });
        Ok(receipt)
    }.await;
    result.map_err(|e: anyhow::Error| e.to_string())
}
#[tauri::command]
pub async fn git_operation_receipts(
    app: tauri::AppHandle,
    window: WebviewWindow,
) -> Result<Value, String> {
    crate::windows::authorize(&window)?;
    let records = ledger(&app)
        .map_err(|e| e.to_string())?
        .list()
        .await
        .map_err(|e| e.to_string())?;
    let state = app.state::<Operations>();
    let live = state.live.lock().unwrap();
    Ok(json!(records.into_iter().rev().map(|mut record|{let active=live.get(&record.operation);if record.status=="pending" && active.is_none(){record.status="needsInspection".into();record.message="No live job owns this pending receipt. Inspect actual Git state; do not automatically retry.".into();}json!({"receipt":record,"active":active.is_some(),"canCancel":active.is_some_and(|job|job.owner==window.label())})}).collect::<Vec<_>>()))
}
#[tauri::command]
pub async fn git_operation_cancel(
    app: tauri::AppHandle,
    window: WebviewWindow,
    operation: String,
) -> Result<(), String> {
    crate::windows::authorize(&window)?;
    let state = app.state::<Operations>();
    let jobs = state.live.lock().unwrap();
    let job = jobs
        .get(&operation)
        .ok_or("This operation is no longer running; inspect its receipt")?;
    if job.owner != window.label() {
        return Err("Cancel the job from its owning window".into());
    }
    job.cancel.send_replace(true);
    Ok(())
}
#[tauri::command]
pub async fn git_operation_inspect(
    app: tauri::AppHandle,
    window: WebviewWindow,
    operation: String,
) -> Result<Value, String> {
    crate::windows::authorize(&window)?;
    let result=async {
        let sessions=app.state::<crate::workspaces::Sessions>();let _gate=sessions.gate.read().await;
        app.state::<crate::windows::Windows>().admit(window.label()).map_err(anyhow::Error::msg)?;
        let state=app.state::<Operations>();let _read=state.reads.acquire().await?;
        anyhow::ensure!(!state.live.lock().unwrap().contains_key(&operation),"Wait for the live job to finish before inspection");
        let ledger=ledger(&app)?;let mut receipt=ledger.list().await?.into_iter().find(|r|r.operation==operation).context("Receipt is unavailable")?;
        let current=operations::catalog(Path::new(&receipt.root)).await?;
        if let Some(before)=&receipt.before {let identity:operations::Identity=serde_json::from_value(current["state"]["identity"].clone())?;anyhow::ensure!(identity==before.identity,"Checkout identity changed; inspect the original operation externally");}
        let remote=if let Some(proof)=&receipt.remote {
            if proof.branch.is_empty(){None}else{
                operations::reconcile_remote(Path::new(&receipt.root),proof).await?
            }
        }else{None};
        receipt.after=serde_json::from_value::<Snapshot>(current["state"].clone()).ok();
        if receipt.status!="completed"{receipt.status="observed".into();receipt.message="Current state inspected; this does not prove attribution, rollback, or success of the interrupted operation. A new action needs a fresh preview.".into();ledger.put(receipt.clone()).await?;}
        Ok(json!({"receipt":receipt,"current":current,"remoteHead":remote}))
    }.await;
    result.map_err(|e: anyhow::Error| e.to_string())
}
fn external_file(app: &tauri::AppHandle) -> Result<PathBuf> {
    Ok(app
        .path()
        .app_data_dir()?
        .join("git-external-leases-v1.json"))
}
pub(crate) async fn external_lease(
    app: &tauri::AppHandle,
    root: &Path,
) -> Result<locks::Held, String> {
    let result=async {
        let identity=operations::identity(root).await?;
        let held=app.state::<Operations>().locks.acquire(&identity,false).await?;
        let file=external_file(app)?;with_file_lock(&file,||async {
            let mut paths:Vec<String>=if file.exists(){anyhow::ensure!(tokio::fs::metadata(&file).await?.len()<=128*1024,"External-tool lease limit reached");serde_json::from_slice(&tokio::fs::read(&file).await?)?}else{vec![]};
            let root=crate::storage::digest(&format!("{:?}",identity.root.as_os_str()));
            if !paths.contains(&root){anyhow::ensure!(paths.len()<256,"External-tool lease limit reached; manage old worktrees with your external Git client");paths.push(root);}
            write_file_atomic(&file,&serde_json::to_string(&paths)?).await
        }).await?;Ok(held)
    }.await;
    result.map_err(|e: anyhow::Error| e.to_string())
}

#[tauri::command]
pub fn git_operation_cancel_read(
    app: tauri::AppHandle,
    window: WebviewWindow,
    request_id: String,
) -> Result<(), String> {
    crate::windows::authorize(&window)?;
    if request_id.len() > 100 {
        return Err("Invalid request ID".into());
    }
    app.state::<Operations>()
        .requests
        .cancel((window.label().into(), request_id));
    Ok(())
}
#[tauri::command]
pub fn git_operation_forget_preview(
    app: tauri::AppHandle,
    window: WebviewWindow,
    preview: String,
) -> Result<(), String> {
    crate::windows::authorize(&window)?;
    let state = app.state::<Operations>();
    let mut previews = state.previews.lock().unwrap();
    if previews
        .get(&preview)
        .is_some_and(|p| p.owner == window.label())
    {
        previews.remove(&preview);
    }
    Ok(())
}
impl Operations {
    pub(crate) fn close_reads(&self, owner: &str) {
        self.requests.cancel_owner(owner);
        self.previews
            .lock()
            .unwrap()
            .retain(|_, p| p.owner != owner);
    }
}

impl Operations {
    fn take_preview(&self, owner: &str, context: Option<&str>, id: &str) -> Result<Preview> {
        let mut previews = self.previews.lock().unwrap();
        let ticket = previews.get(id).context(
            "Preview was used or expired. Check Operations before preparing another action.",
        )?;
        anyhow::ensure!(
            ticket.owner == owner
                && ticket.context.as_deref() == context
                && ticket.created.elapsed() < Duration::from_secs(300),
            "Preview belongs to another window/context or expired"
        );
        Ok(previews.remove(id).unwrap())
    }
}
fn intent(action: &Action) -> Result<Value> {
    let mut value = serde_json::to_value(action)?;
    if let Some(fields) = value.as_object_mut() {
        fields.remove("message");
        fields.remove("url");
        fields.remove("content");
        fields.remove("expected");
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ticket(owner: &str, context: &str) -> Preview {
        let identity = operations::Identity {
            root: "/fixture".into(),
            git: "/fixture/.git".into(),
            common: "/fixture/.git".into(),
            inodes: vec![(1, 2)],
        };
        Preview {
            owner: owner.into(),
            context: Some(context.into()),
            created: Instant::now(),
            bytes: 10,
            plan: Prepared::Repository(Box::new(Plan {
                before: Snapshot {
                    identity,
                    fingerprint: "fingerprint".into(),
                    stash_fingerprint: String::new(),
                    head: None,
                    branch: None,
                    operations: vec![],
                    conflicts: false,
                },
                action: Action::Stage {
                    path: "file".into(),
                },
                steps: vec![],
                preview: String::new(),
                patch: None,
                remote: None,
                extra: vec![],
                destination: None,
            })),
        }
    }
    #[test]
    fn resolution_receipts_exclude_edited_content_and_proof() {
        let value = intent(&Action::ResolveText {
            path: "file".into(),
            content: "private resolution text".into(),
            expected: "proof".into(),
        })
        .unwrap();
        assert_eq!(value["path"], "file");
        assert!(value.get("content").is_none() && value.get("expected").is_none());
    }
    #[test]
    fn preview_authority_requires_the_exact_window_and_context_and_consumes_once() {
        let state = Operations::default();
        state
            .previews
            .lock()
            .unwrap()
            .insert("one".into(), ticket("main", "context-a"));
        assert!(state
            .take_preview("peer", Some("context-a"), "one")
            .is_err());
        assert!(state
            .take_preview("main", Some("context-b"), "one")
            .is_err());
        assert!(state.take_preview("main", Some("context-a"), "one").is_ok());
        assert!(state
            .take_preview("main", Some("context-a"), "one")
            .is_err());
    }
    #[tokio::test]
    async fn closing_a_window_cancels_only_reads_and_keeps_native_write_jobs_owned() {
        let state = Operations::default();
        state
            .previews
            .lock()
            .unwrap()
            .insert("one".into(), ticket("main", "context-a"));
        let (_guard, cancelled) = state
            .requests
            .start(("main".into(), "read".into()))
            .unwrap();
        let (cancel, receiver) = watch::channel(false);
        state.live.lock().unwrap().insert(
            "write".into(),
            Live {
                owner: "main".into(),
                cancel,
            },
        );
        state.close_reads("main");
        assert!(tokio::time::timeout(Duration::from_secs(1), cancelled)
            .await
            .is_ok());
        assert!(!*receiver.borrow());
        assert!(state.live.lock().unwrap().contains_key("write"));
        assert!(state.previews.lock().unwrap().is_empty());
    }
}

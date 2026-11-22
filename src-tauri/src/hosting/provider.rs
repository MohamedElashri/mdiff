use super::{
    http::{Broker, Connection},
    types::*,
};
use anyhow::{bail, Result};
use reqwest::Method;
use serde_json::{json, Value};
fn paged(n: u32) -> Result<Vec<(&'static str, String)>> {
    Ok(vec![
        ("per_page", "20".into()),
        ("page", page(n)?.to_string()),
    ])
}
fn rows(value: &Value, key: &str) -> Vec<Value> {
    items(if key.is_empty() { value } else { &value[key] })
}
fn actor(v: &Value) -> String {
    v.get("login")
        .or_else(|| v.get("username"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned()
}
fn provider_id(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_u64().map(|id| id.to_string()))
}
fn summary(r: ReviewRef, v: &Value) -> Value {
    let gh = r.site.provider == Provider::Github;
    json!({"review":r,"title":text(v,"title"),"body":text(v,if gh{"body"}else{"description"}),"state":text(v,"state"),"author":actor(&v[if gh{"user"}else{"author"}]),"head":if gh{v.pointer("/head/sha").cloned().unwrap_or(Value::Null)}else{v.get("sha").cloned().unwrap_or(Value::Null)},"url":r.url()})
}
impl Broker {
    pub async fn user(&self, c: &Connection) -> Result<Value> {
        self.read(c, Method::GET, "/user", &[], None, false).await
    }
    pub async fn discover(&self, c: &Connection, scope: &str, n: u32) -> Result<Value> {
        if !["authored", "requested"].contains(&scope) {
            bail!("Unknown review list");
        }
        if c.token.is_none() {
            bail!("Connect this host to discover your reviews");
        }
        let mut args = paged(n)?;
        let (path, key) = match c.site.provider {
            Provider::Github => {
                args.push((
                    "q",
                    format!(
                        "is:pr is:open {}:@me",
                        if scope == "authored" {
                            "author"
                        } else {
                            "review-requested"
                        }
                    ),
                ));
                args.push(("sort", "updated".into()));
                ("/search/issues", "items")
            }
            Provider::Gitlab => {
                let user = self.user(c).await?;
                args.extend([
                    ("state", "opened".into()),
                    ("scope", "all".into()),
                    (
                        if scope == "authored" {
                            "author_id"
                        } else {
                            "reviewer_id"
                        },
                        user["id"]
                            .as_u64()
                            .ok_or_else(|| anyhow::anyhow!("Cannot identify GitLab account"))?
                            .to_string(),
                    ),
                ]);
                ("/merge_requests", "")
            }
        };
        let data = self.read(c, Method::GET, path, &args, None, false).await?;
        let mut reviews = vec![];
        for value in rows(&data, key) {
            let url = value
                .get("html_url")
                .or_else(|| value.get("web_url"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let Ok(reference) = ReviewRef::parse(url, c.site.provider) else {
                continue;
            };
            if reference.site != c.site {
                continue;
            }
            reviews.push(summary(reference, &value));
        }
        Ok(
            json!({"items":reviews,"page":n,"next":(reviews.len()==20).then_some(n+1),"incomplete":data.get("incomplete_results").and_then(Value::as_bool).unwrap_or(false)}),
        )
    }
    pub async fn details(&self, c: &Connection, r: &ReviewRef, fresh: bool) -> Result<Value> {
        let raw = self
            .read(c, Method::GET, &c.review_path(r), &[], None, fresh)
            .await?;
        let mut value = summary(r.clone(), &raw);
        let gh = c.site.provider == Provider::Github;
        value["head"] = if gh {
            raw.pointer("/head/sha").cloned().unwrap_or(Value::Null)
        } else {
            raw.get("sha")
                .or_else(|| raw.pointer("/diff_refs/head_sha"))
                .cloned()
                .unwrap_or(Value::Null)
        };
        value["base"] = if gh {
            raw.pointer("/base/sha").cloned().unwrap_or(Value::Null)
        } else {
            raw.pointer("/diff_refs/base_sha")
                .cloned()
                .unwrap_or(Value::Null)
        };
        value["mergeRevision"] = if gh {
            raw.get("merge_commit_sha").cloned().unwrap_or(Value::Null)
        } else {
            raw.pointer("/head_pipeline/sha")
                .cloned()
                .unwrap_or(Value::Null)
        };
        value["merged"] = json!(raw["merged"]
            .as_bool()
            .unwrap_or_else(|| raw["state"] == "merged"));
        value["draft"] = json!(raw["draft"]
            .as_bool()
            .unwrap_or_else(|| raw["work_in_progress"].as_bool().unwrap_or(false)));
        value["mergeStatus"] = if gh {
            raw.get("mergeable_state").cloned().unwrap_or(Value::Null)
        } else {
            raw.get("detailed_merge_status")
                .cloned()
                .unwrap_or(Value::Null)
        };
        value["mergeCommit"] = raw.get("merge_commit_sha").cloned().unwrap_or(Value::Null);
        value["nodeId"] = raw.get("node_id").cloned().unwrap_or(Value::Null);
        let methods = if gh {
            let repo = self
                .read(c, Method::GET, &c.project(r), &[], None, false)
                .await?;
            let writable = repo
                .pointer("/permissions/push")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            [
                ("merge", "allow_merge_commit"),
                ("squash", "allow_squash_merge"),
                ("rebase", "allow_rebase_merge"),
            ]
            .into_iter()
            .filter(|(_, key)| writable && repo[*key] == true)
            .map(|(method, _)| method)
            .collect::<Vec<_>>()
        } else {
            let project = self
                .read(c, Method::GET, &c.project(r), &[], None, false)
                .await?;
            let writable = raw
                .pointer("/user/can_merge")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let policy = project["squash_option"].as_str().unwrap_or("never");
            let mut methods = vec![];
            if writable {
                if policy != "always" {
                    methods.push("merge");
                }
                if policy != "never" {
                    methods.push("squash");
                }
            }
            methods
        };
        value["capabilities"] = json!({"comment":c.token.is_some(),"reply":c.token.is_some(),"review":if gh{vec!["comment","approve","requestChanges"]}else{vec!["comment","approve"]},"mergeMethods":if c.token.is_some(){methods}else{vec![]},"resolve":c.token.is_some()});
        Ok(value)
    }
    pub async fn checks(
        &self,
        c: &Connection,
        r: &ReviewRef,
        revision: &str,
        n: u32,
    ) -> Result<Value> {
        sha(revision)?;
        let mut args = paged(n)?;
        let project = c.project(r);
        let mut result = vec![];
        let mut warnings = vec![];
        if c.site.provider == Provider::Github {
            let path = format!("{project}/commits/{revision}/check-runs");
            args.push(("filter", "all".into()));
            match self.read(c, Method::GET, &path, &args, None, false).await {
                Ok(value) => {
                    for v in rows(&value, "check_runs") {
                        if v["head_sha"] != revision {
                            continue;
                        }
                        result.push(json!({"id":format!("check:{}",v["id"]),"kind":"check","name":text(&v,"name"),"status":v.get("conclusion").filter(|v|!v.is_null()).unwrap_or(&v["status"]),"sha":revision,"url":link(&v,"html_url").or_else(||link(&v,"details_url")),"run":null,"attempt":null}));
                    }
                }
                Err(e) => warnings.push(e.to_string()),
            }
            match self
                .read(
                    c,
                    Method::GET,
                    &format!("{project}/commits/{revision}/statuses"),
                    &paged(n)?,
                    None,
                    false,
                )
                .await
            {
                Ok(value) => {
                    for v in items(&value) {
                        result.push(json!({"id":format!("status:{}",v["id"]),"kind":"status","name":text(&v,"context"),"status":text(&v,"state"),"sha":revision,"url":link(&v,"target_url"),"run":null,"attempt":null}));
                    }
                }
                Err(e) => warnings.push(e.to_string()),
            }
            let mut args = paged(n)?;
            args.push(("head_sha", revision.into()));
            match self
                .read(
                    c,
                    Method::GET,
                    &format!("{project}/actions/runs"),
                    &args,
                    None,
                    false,
                )
                .await
            {
                Ok(value) => {
                    for v in rows(&value, "workflow_runs") {
                        if v["head_sha"] != revision {
                            continue;
                        }
                        result.push(json!({"id":format!("run:{}:{}",v["id"],v["run_attempt"]),"kind":"workflow","name":text(&v,"name"),"status":v.get("conclusion").filter(|v|!v.is_null()).unwrap_or(&v["status"]),"sha":revision,"url":link(&v,"html_url"),"run":v["id"],"attempt":v["run_attempt"]}));
                    }
                }
                Err(e) => warnings.push(e.to_string()),
            }
        } else {
            let data = self
                .read(
                    c,
                    Method::GET,
                    &format!("{}/pipelines", c.review_path(r)),
                    &args,
                    None,
                    false,
                )
                .await?;
            for v in items(&data) {
                if v["sha"] != revision {
                    continue;
                }
                result.push(json!({"id":format!("pipeline:{}",v["id"]),"kind":"pipeline","name":format!("Pipeline {}",v["id"]),"status":text(&v,"status"),"sha":revision,"url":link(&v,"web_url"),"run":v["id"],"attempt":null}));
            }
        }
        Ok(
            json!({"sha":revision,"items":result,"warnings":warnings,"page":n,"next":(result.len()>=20).then_some(n+1),"complete":false}),
        )
    }
    pub async fn conversation(
        &self,
        c: &Connection,
        r: &ReviewRef,
        n: u32,
        fresh: bool,
    ) -> Result<Value> {
        let path = match c.site.provider {
            Provider::Github => format!("{}/issues/{}/comments", c.project(r), r.number),
            Provider::Gitlab => format!("{}/notes", c.review_path(r)),
        };
        let data = self
            .read(c, Method::GET, &path, &paged(n)?, None, fresh)
            .await?;
        let values = items(&data);
        let notes=values.iter().filter(|v|v["system"]!=true).map(|v|json!({"id":provider_id(&v["id"]),"body":text(v,"body"),"author":actor(&v[if c.site.provider==Provider::Github{"user"}else{"author"}]),"authorId":v[if c.site.provider==Provider::Github{"user"}else{"author"}]["id"],"url":link(v,"html_url"),"created":text(v,"created_at")})).collect::<Vec<_>>();
        Ok(json!({"items":notes,"page":n,"next":(values.len()==20).then_some(n+1)}))
    }
    pub async fn threads(
        &self,
        c: &Connection,
        r: &ReviewRef,
        n: u32,
        cursor: Option<&str>,
    ) -> Result<Value> {
        page(n)?;
        if c.site.provider == Provider::Gitlab {
            let data = self
                .read(
                    c,
                    Method::GET,
                    &format!("{}/discussions", c.review_path(r)),
                    &paged(n)?,
                    None,
                    false,
                )
                .await?;
            let values = items(&data);
            let threads=values.iter().filter(|v|v["individual_note"]!=true).map(|v|{
                let notes=items(&v["notes"]);let first=notes.first().cloned().unwrap_or(Value::Null);
                json!({"id":text(v,"id"),"resolved":first["resolved"],"canResolve":first["resolvable"],"outdated":null,"positionSha":first.pointer("/position/head_sha"),"oldPath":first.pointer("/position/old_path"),"newPath":first.pointer("/position/new_path"),"oldLine":first.pointer("/position/old_line"),"newLine":first.pointer("/position/new_line"),"path":first.pointer("/position/new_path").or_else(||first.pointer("/position/old_path")),"line":first.pointer("/position/new_line").or_else(||first.pointer("/position/old_line")),"truncated":notes.len()>20,"comments":notes.iter().take(20).map(|note|json!({"id":provider_id(&note["id"]),"body":text(note,"body"),"author":actor(&note["author"]),"authorId":note["author"]["id"],"url":null})).collect::<Vec<_>>()})
            }).collect::<Vec<_>>();
            return Ok(
                json!({"items":threads,"page":n,"next":(values.len()==20).then_some(n+1),"cursor":null}),
            );
        }
        if let Some(cursor) = cursor {
            if cursor.len() > 1024 {
                bail!("Invalid discussion cursor");
            }
        }
        let (owner, name) = r.repository.split_once('/').unwrap();
        let data=self.graphql(c,"query($owner:String!,$name:String!,$number:Int!,$after:String){repository(owner:$owner,name:$name){pullRequest(number:$number){reviewThreads(first:20,after:$after){pageInfo{hasNextPage endCursor} nodes{id isResolved isOutdated viewerCanResolve viewerCanUnresolve path line originalLine diffSide comments(first:20){pageInfo{hasNextPage} nodes{databaseId:fullDatabaseId body url author{login} originalCommit{oid} commit{oid}}}}}}}}",json!({"owner":owner,"name":name,"number":r.number,"after":cursor}),false).await?;
        let connection = &data["repository"]["pullRequest"]["reviewThreads"];
        if connection.is_null() {
            bail!("GitHub discussion access is unavailable");
        }
        let threads=items(&connection["nodes"]).iter().map(|v|json!({"id":text(v,"id"),"resolved":v["isResolved"],"outdated":v["isOutdated"],"canResolve":if v["isResolved"]==true{&v["viewerCanUnresolve"]}else{&v["viewerCanResolve"]},"path":v["path"],"line":v["line"],"originalLine":v["originalLine"],"side":v["diffSide"],"truncated":v["comments"]["pageInfo"]["hasNextPage"],"comments":items(&v["comments"]["nodes"]).iter().map(|note|json!({"id":provider_id(&note["databaseId"]),"body":text(note,"body"),"author":note["author"]["login"],"url":link(note,"url"),"sha":note["originalCommit"]["oid"],"currentSha":note["commit"]["oid"]})).collect::<Vec<_>>()})).collect::<Vec<_>>();
        Ok(
            json!({"items":threads,"page":n,"next":(connection["pageInfo"]["hasNextPage"]==true).then_some(n+1),"cursor":connection["pageInfo"]["endCursor"]}),
        )
    }
    pub async fn jobs(
        &self,
        c: &Connection,
        r: &ReviewRef,
        revision: &str,
        run: u64,
        attempt: Option<u32>,
        n: u32,
    ) -> Result<Value> {
        sha(revision)?;
        if run == 0 || attempt.is_some_and(|a| a == 0 || a > 1000) {
            bail!("Invalid CI run");
        }
        let project = c.project(r);
        let (path, key) = if c.site.provider == Provider::Github {
            let meta = self
                .read(
                    c,
                    Method::GET,
                    &format!("{project}/actions/runs/{run}"),
                    &[],
                    None,
                    false,
                )
                .await?;
            if meta["head_sha"] != revision {
                bail!("Workflow run belongs to a different commit");
            }
            let attempt = attempt
                .or_else(|| meta["run_attempt"].as_u64().map(|v| v as u32))
                .ok_or_else(|| anyhow::anyhow!("Run attempt is unavailable"))?;
            (
                format!("{project}/actions/runs/{run}/attempts/{attempt}/jobs"),
                "jobs",
            )
        } else {
            let meta = self
                .read(
                    c,
                    Method::GET,
                    &format!("{project}/pipelines/{run}"),
                    &[],
                    None,
                    false,
                )
                .await?;
            if meta["sha"] != revision {
                bail!("Pipeline belongs to a different commit");
            }
            (format!("{project}/pipelines/{run}/jobs"), "")
        };
        let data = self
            .read(c, Method::GET, &path, &paged(n)?, None, false)
            .await?;
        let values = rows(&data, key);
        let jobs=values.iter().map(|v|json!({"id":v["id"],"name":text(v,"name"),"status":v.get("conclusion").filter(|v|!v.is_null()).unwrap_or(&v["status"]),"url":link(v,"html_url").or_else(||link(v,"web_url")),"steps":items(&v["steps"]).iter().take(100).map(|s|json!({"name":text(s,"name"),"status":s.get("conclusion").filter(|v|!v.is_null()).unwrap_or(&s["status"])})).collect::<Vec<_>>()})).collect::<Vec<_>>();
        Ok(
            json!({"items":jobs,"page":n,"next":(values.len()==20).then_some(n+1),"sha":revision,"run":run,"attempt":attempt}),
        )
    }
    pub async fn job_log(
        &self,
        c: &Connection,
        r: &ReviewRef,
        revision: &str,
        job: u64,
    ) -> Result<Value> {
        sha(revision)?;
        if job == 0 {
            bail!("Invalid CI job");
        }
        let project = c.project(r);
        let path = if c.site.provider == Provider::Github {
            format!("{project}/actions/jobs/{job}")
        } else {
            format!("{project}/jobs/{job}")
        };
        let meta = self.read(c, Method::GET, &path, &[], None, false).await?;
        let actual = if c.site.provider == Provider::Github {
            meta.get("head_sha")
        } else {
            meta.pointer("/commit/id")
        };
        if actual.and_then(Value::as_str) != Some(revision) {
            bail!("Job belongs to a different commit or its revision is unavailable");
        }
        self.log(
            c,
            &format!(
                "{path}/{}",
                if c.site.provider == Provider::Github {
                    "logs"
                } else {
                    "trace"
                }
            ),
        )
        .await
    }
    pub async fn graphql(
        &self,
        c: &Connection,
        query: &str,
        variables: Value,
        write: bool,
    ) -> Result<Value> {
        let body = json!({"query":query,"variables":variables});
        let value = if write {
            self.write(c, Method::POST, "/graphql", body).await?
        } else {
            self.read(c, Method::POST, "/graphql", &[], Some(body), false)
                .await?
        };
        if value["errors"].as_array().is_some_and(|v| !v.is_empty()) {
            bail!("GitHub could not complete this GraphQL operation; check permissions and reconcile writes before retrying");
        }
        Ok(value["data"].clone())
    }
}

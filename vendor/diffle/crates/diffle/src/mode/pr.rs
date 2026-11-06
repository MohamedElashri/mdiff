//! A GitHub pull request as a range, fetched into refs this run owns.
use super::resolve::resolve_range;
use crate::error::{Error, GithubError, Result};
use crate::git::GitRepo;
use crate::github::client::no_token;
use crate::github::hosts::{Github, GithubInstance, github_repository};
use crate::github::pulls::{PullRequest, view_pr};
use crate::protocol::{ComparisonBase, PullRef, RangeView, Review};

/// A pull request, as GitHub shows it: merge-base(base tip, head) vs head. GitHub names it, then
/// one fetch brings the base tip and the head into session-owned refs named after the branches, so
/// a pull request nobody has checked out is reviewable. The refs stay fixed until another fetch;
/// the range carries the pull request's identity, which keys its comments like the branches would.
pub async fn resolve_pr(selector: Option<&str>, repo: &GitRepo, github: &Github) -> Result<Review> {
    let Some(client) = &github.client else {
        return Err(GithubError::conflict(no_token(&github.instance.host)).into());
    };
    let pr = view_pr(selector, Some(repo), client.as_ref(), &github.instance, None).await?;
    let head = format!("{}/{}/head/{}", repo.review_refs, pr.number, pr.head_ref_name);
    let base = format!("{}/{}/base/{}", repo.review_refs, pr.number, pr.base_ref_name);
    let refspecs =
        [format!("+refs/pull/{}/head:{head}", pr.number), format!("+refs/heads/{}:{base}", pr.base_ref_name)];
    repo.fetch(&fetch_source(repo, &pr, &github.instance).await?, &refspecs).await?;
    let (actual_head, actual_base) = tokio::try_join!(repo.resolve(&head), repo.resolve(&base))?;
    if actual_head != pr.head_ref_oid || actual_base != pr.base_ref_oid {
        return Err(GithubError::conflict(
            "The fetched commits do not match the pull request. It may have changed; reopen the review.",
        )
        .into());
    }
    resolve_range(repo, &base, &head, ComparisonBase::MergeBase).await?;
    Ok(Review::Range {
        old: base,
        new: head,
        base: ComparisonBase::MergeBase,
        pr: Some(PullRef {
            url: pr.url,
            number: pr.number,
            repository: pr.repository,
            head_repository: pr.head_repository,
        }),
        view: RangeView::Whole,
    })
}

/// `refs/pull/*` lives on the base repository, so the fetch goes to the remote that points at it.
/// Foreign repositories are opened in a clone by the CLI instead.
async fn fetch_source(repo: &GitRepo, pr: &PullRequest, instance: &GithubInstance) -> Result<String> {
    let remotes = repo.remotes().await?;
    match remotes
        .into_iter()
        .find(|r| github_repository(&r.url, instance).is_some_and(|name| name.eq_ignore_ascii_case(&pr.repository)))
    {
        // An entered token also works when the local remote uses SSH and the
        // user has no SSH credentials. Its scope is the canonical HTTPS host.
        Some(_) if repo.has_git_auth() => Ok(instance.repository_url(&pr.repository)),
        Some(remote) => Ok(remote.name),
        None => Err(Error::revspec(format!("foreign repository; open it with diffle pr {}", pr.url))),
    }
}

//! Comparing two ranges: one's head replayed onto the other's base, and their commits paired.
use crate::error::{Error, Result};
use crate::git::GitRepo;
use crate::protocol::{MAX_RANGE_COMMITS, PairStatus, RangeCommit, RangeEnds, RangePair, range_label};

/// `older`'s head as it would be on `newer`'s base: the tree to show `newer`'s head against, so only
/// what the series itself changed in between shows, and the paths the replay could not merge.
/// The same base needs no replay: the older head's tree is what was reviewed.
pub async fn replay_range(repo: &GitRepo, older: &RangeEnds, newer: &RangeEnds) -> Result<(String, Vec<String>)> {
    if older.old_sha == newer.old_sha {
        return Ok((repo.tree(&older.new_sha).await?, Vec::new()));
    }
    Ok(repo.replay(&older.old_sha, &newer.old_sha, &older.new_sha).await?)
}

/// Pairs the two ranges' commits as `git range-diff` does and tells a pair whose patch is the same
/// (a reworded commit) from an amended one by patch id. range-diff compares every commit with
/// every other; larger ranges use fingerprint matching and bounded candidate comparisons.
pub async fn pair_ranges(repo: &GitRepo, older: &RangeEnds, newer: &RangeEnds) -> Result<Vec<RangePair>> {
    let old_range = format!("{}..{}", older.old_sha, older.new_sha);
    let new_range = format!("{}..{}", newer.old_sha, newer.new_sha);
    let (old_commits, new_commits) =
        tokio::try_join!(repo.range_commits(&old_range, 5000), repo.range_commits(&new_range, 5000))?;
    for (side, commits) in [(older, &old_commits), (newer, &new_commits)] {
        if commits.total > 5000 {
            return Err(Error::revspec(format!(
                "{} spans {} commits; a range comparison takes at most 5000",
                range_label(&side.old_sha, &side.new_sha),
                commits.total
            )));
        }
    }
    if old_commits.total > MAX_RANGE_COMMITS as i64 || new_commits.total > MAX_RANGE_COMMITS as i64 {
        return tokio::time::timeout(
            std::time::Duration::from_secs(60),
            pair_large(repo, old_commits.list, new_commits.list),
        )
        .await
        .map_err(|_| Error::revspec("Large-range comparison exceeded 60 seconds; compare a smaller range"))?;
    }
    let rows = repo.range_diff(&old_range, &new_range).await?;
    // range-diff abbreviates; a commit beyond the listed window is looked up on its own.
    let find = |list: &[RangeCommit], abbrev: Option<String>| {
        let listed = abbrev.as_ref().and_then(|a| list.iter().find(|c| c.sha.starts_with(a.as_str())).cloned());
        async move {
            match (listed, abbrev) {
                (Some(commit), _) => Ok(Some(commit)),
                (None, Some(abbrev)) => {
                    Ok::<_, Error>(repo.range_commits(&format!("{abbrev}^!"), 1).await?.list.into_iter().next())
                }
                (None, None) => Ok(None),
            }
        }
    };
    let paired = futures::future::try_join_all(rows.into_iter().map(|row| {
        let (old, new) = (find(&old_commits.list, row.old), find(&new_commits.list, row.new));
        async move { Ok::<_, Error>((old.await?, new.await?, row.marker)) }
    }))
    .await?;
    let changed: Vec<String> = paired
        .iter()
        .filter_map(|(old, new, marker)| match (old, new) {
            (Some(old), Some(new)) if *marker == '!' => Some([old.sha.clone(), new.sha.clone()]),
            _ => None,
        })
        .flatten()
        .collect();
    let ids = repo.patch_ids(&changed).await?;
    Ok(paired
        .into_iter()
        .map(|(old, new, marker)| {
            let status = match marker {
                '=' => PairStatus::Identical,
                '<' => PairStatus::Dropped,
                '>' => PairStatus::Added,
                _ => match (&old, &new) {
                    (Some(o), Some(n)) if ids.get(&o.sha).is_some_and(|id| ids.get(&n.sha) == Some(id)) => {
                        PairStatus::Message
                    }
                    _ => PairStatus::Changed,
                },
            };
            RangePair { old, new, status }
        })
        .collect())
}

/// Large series match exact identities first. Only small, unique candidate groups ask Git to
/// compare patches; ambiguous matches remain added/dropped instead of being assigned arbitrarily.
async fn pair_large(repo: &GitRepo, old: Vec<RangeCommit>, new: Vec<RangeCommit>) -> Result<Vec<RangePair>> {
    use std::collections::{BTreeMap, HashMap, HashSet};
    let mut used = HashSet::new();
    let mut matches = HashMap::new();
    let by_sha: HashMap<&str, usize> = old.iter().enumerate().map(|(i, c)| (c.sha.as_str(), i)).collect();
    for (j, c) in new.iter().enumerate() {
        if let Some(&i) = by_sha.get(c.sha.as_str()) {
            used.insert(i);
            matches.insert(j, (i, PairStatus::Identical));
        }
    }
    let shas: Vec<String> = old
        .iter()
        .enumerate()
        .filter(|(i, _)| !used.contains(i))
        .map(|(_, c)| c.sha.clone())
        .chain(new.iter().enumerate().filter(|(j, _)| !matches.contains_key(j)).map(|(_, c)| c.sha.clone()))
        .collect();
    let ids = repo.patch_ids(&shas).await?;
    let mut old_ids: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut new_ids: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, c) in old.iter().enumerate().filter(|(i, _)| !used.contains(i)) {
        if let Some(id) = ids.get(&c.sha) {
            old_ids.entry(id).or_default().push(i);
        }
    }
    for (j, c) in new.iter().enumerate().filter(|(j, _)| !matches.contains_key(j)) {
        if let Some(id) = ids.get(&c.sha) {
            new_ids.entry(id).or_default().push(j);
        }
    }
    for (id, before) in old_ids {
        if before.len() == 1
            && let Some(after) = new_ids.get(id).filter(|v| v.len() == 1)
        {
            let (i, j) = (before[0], after[0]);
            used.insert(i);
            matches.insert(
                j,
                (i, if old[i].message == new[j].message { PairStatus::Identical } else { PairStatus::Message }),
            );
        }
    }
    let mut subjects: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut next_subjects: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, c) in old.iter().enumerate().filter(|(i, _)| !used.contains(i)) {
        subjects.entry(c.message.lines().next().unwrap_or_default().to_owned()).or_default().push(i);
    }
    for (j, c) in new.iter().enumerate().filter(|(j, _)| !matches.contains_key(j)) {
        next_subjects.entry(c.message.lines().next().unwrap_or_default().to_owned()).or_default().push(j);
    }
    let mut comparisons = 0;
    for (subject, before) in subjects {
        let Some(after) = next_subjects.get(&subject) else { continue };
        if subject.is_empty() || before.len() * after.len() > 16 {
            continue;
        }
        let mut edges = Vec::new();
        for &i in &before {
            for &j in after {
                if comparisons >= 128 {
                    continue;
                }
                comparisons += 1;
                let rows = repo.range_diff(&format!("{}^!", old[i].sha), &format!("{}^!", new[j].sha)).await?;
                if rows.iter().any(|r| matches!(r.marker, '=' | '!')) {
                    edges.push((i, j));
                }
            }
        }
        for &(i, j) in &edges {
            if comparisons < 128
                && edges.iter().filter(|(a, _)| *a == i).count() == 1
                && edges.iter().filter(|(_, b)| *b == j).count() == 1
            {
                used.insert(i);
                matches.insert(j, (i, PairStatus::Changed));
            }
        }
    }
    let mut result = Vec::new();
    for (j, c) in new.into_iter().enumerate() {
        let (previous, status) =
            matches.remove(&j).map_or((None, PairStatus::Added), |(i, status)| (Some(old[i].clone()), status));
        result.push(RangePair { old: previous, new: Some(c), status });
    }
    for (i, c) in old.into_iter().enumerate() {
        if !used.contains(&i) {
            result.push(RangePair { old: Some(c), new: None, status: PairStatus::Dropped });
        }
    }
    Ok(result)
}

//! Re-anchoring threads as the reviewed files change.
use std::collections::HashMap;
use std::future::Future;

use super::anchor::{Context, LineIndex};
use super::hunks::is_shown;
use super::store::{CommentStore, read_set, write_set};
use crate::error::{Error, Result};
use crate::persist::with_file_lock;
use crate::protocol::{CommentAnchor, CommentThread, Side};

/// One side of a file as the review presents it.
pub struct SideView {
    pub contents: String,
    /// Line ranges on display (a diff's hunks); none when the whole side is shown.
    pub shown: Option<Vec<(i64, i64)>>,
}

/// The review as relocation sees it.
pub trait ReviewView: Sync {
    /// One side of a path; none when the review has no such side.
    fn side(&self, path: &str, side: Side) -> impl Future<Output = Result<Option<SideView>>> + Send;
    /// Whether the review has a file at `path` on either side, which is all a file thread needs.
    fn has_file(&self, path: &str) -> bool;
    fn renamed(&self, _path: &str) -> Option<String> {
        None
    }
    fn affected(&self, _path: &str) -> bool {
        true
    }
}

impl CommentStore {
    /// Re-anchors every thread against the current review. A line thread is stale when its side is
    /// gone, its text is not found, or the relocated range falls outside what the review shows; a
    /// file thread when its file left the review. Returns whether anything changed.
    pub async fn relocate_all(&self, review: &impl ReviewView) -> Result<bool> {
        // Under the lock: a thread added or moved by another store meanwhile is re-anchored too, never clobbered.
        with_file_lock(&self.file, || async {
            let mut set = read_set(&self.file, &self.key).await?;
            let mut sides: HashMap<(Side, String), Option<(SideView, LineIndex)>> = HashMap::new();
            let mut changed = false;
            for t in &mut set.threads {
                let path = match &t.anchor {
                    CommentAnchor::File { path } | CommentAnchor::Line { path, .. } => path,
                };
                if !review.affected(path) {
                    continue;
                }
                if let Some(renamed) = review.renamed(path) {
                    match &mut t.anchor {
                        CommentAnchor::File { path } | CommentAnchor::Line { path, .. } => *path = renamed,
                    }
                    changed = true;
                }
                changed |= match &t.anchor {
                    CommentAnchor::File { path } => {
                        let stale = !review.has_file(path);
                        std::mem::replace(&mut t.stale, stale) != stale
                    }
                    CommentAnchor::Line { path, side, .. } => {
                        let key = (*side, path.clone());
                        if !sides.contains_key(&key) {
                            match review.side(path, *side).await {
                                Ok(view) => sides.insert(
                                    key.clone(),
                                    view.map(|v| {
                                        let index = LineIndex::new(&v.contents);
                                        (v, index)
                                    }),
                                ),
                                Err(e) => return Ok(Err(e)),
                            };
                        }
                        let moved = relocate_thread(t, sides[&key].as_ref(), set.contexts.get(&t.id));
                        if !t.stale
                            && let Some((_, index)) = &sides[&key]
                            && let CommentAnchor::Line { start_line, end_line, .. } = &t.anchor
                        {
                            let context = index.context(*start_line, *end_line);
                            if set.contexts.get(&t.id) != Some(&context) {
                                set.contexts.insert(t.id.clone(), context);
                                changed = true;
                            }
                        }
                        moved
                    }
                };
            }
            if changed {
                write_set(&self.file, &self.key, set).await?;
            }
            Ok(Ok::<_, Error>(changed))
        })
        .await?
    }
}

/// Moves a line thread onto its text in `view`, or marks it stale. Returns whether it changed.
fn relocate_thread(t: &mut CommentThread, view: Option<&(SideView, LineIndex)>, context: Option<&Context>) -> bool {
    let CommentAnchor::Line { start_line, end_line, quoted, .. } = &mut t.anchor else { return false };
    let moved = view
        .and_then(|(v, index)| index.locate(quoted, context).map(|range| (range, v)))
        .filter(|((s, e), v)| v.shown.as_ref().is_none_or(|shown| is_shown(shown, *s, *e)))
        .map(|(range, _)| range);
    let Some((start, end)) = moved else {
        if t.stale {
            return false;
        }
        t.stale = true;
        t.stale_from_line = Some(*start_line);
        return true;
    };
    let mut changed = false;
    if t.stale {
        t.stale = false;
        t.stale_from_line = None;
        changed = true;
    }
    if (start, end) != (*start_line, *end_line) {
        (*start_line, *end_line) = (start, end);
        changed = true;
    }
    changed
}

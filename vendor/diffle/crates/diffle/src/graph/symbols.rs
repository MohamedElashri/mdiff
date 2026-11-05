//! Which symbols of a file outline make up its interface, and which of them a patch touches.
use super::select::Changed;
use crate::comments::hunks::NewSideChanges;
use crate::protocol::{GraphNodeStatus, LspSymbol};

/// LSP `SymbolKind`s that name an interface rather than an implementation detail: Class, Method,
/// Constructor, Enum, Interface, Function, Struct and TypeParameter (rust-analyzer's type aliases).
/// Constants and variables are left out: they are many, and their type says little about the design.
/// Each server maps its language onto these, so the choice holds for every language.
const INTERFACE_KINDS: [i64; 8] = [5, 6, 9, 10, 11, 12, 23, 26];
/// Kinds that only group declarations: Module, Namespace, Package, Class, Enum, Interface, Object
/// (rust-analyzer's `impl`) and Struct. Anything inside another kind, such as a function's local
/// constant or nested `def`, is its implementation.
const NAMESPACE_KINDS: [i64; 8] = [2, 3, 4, 5, 10, 11, 19, 23];

/// The interface-level symbols of a file outline: kinds in `INTERFACE_KINDS`, enclosed only by
/// namespace kinds. Every other function here takes such an outline.
pub fn interface(outline: Vec<LspSymbol>) -> Vec<LspSymbol> {
    // Normal LSP outlines are parent-first, ordered, nested ranges. A stack handles
    // those in linear time; malformed/overlapping outlines retain the exact fallback.
    let mut stack: Vec<(i64, bool)> = Vec::new();
    let mut keep = Vec::with_capacity(outline.len());
    let mut previous = i64::MIN;
    let mut ordered = true;
    for symbol in &outline {
        if symbol.start_line < previous {
            ordered = false;
            break;
        }
        previous = symbol.start_line;
        while stack.last().is_some_and(|(end, _)| *end < symbol.start_line) {
            stack.pop();
        }
        if stack.last().is_some_and(|(end, _)| *end < symbol.end_line) {
            ordered = false;
            break;
        }
        let blocked = stack.last().is_some_and(|(_, blocked)| *blocked);
        keep.push(INTERFACE_KINDS.contains(&symbol.kind) && !blocked);
        stack.push((symbol.end_line, blocked || !NAMESPACE_KINDS.contains(&symbol.kind)));
    }
    if !ordered {
        keep = outline
            .iter()
            .enumerate()
            .map(|(i, symbol)| {
                INTERFACE_KINDS.contains(&symbol.kind)
                    && outline.iter().enumerate().all(|(j, parent)| {
                        j >= i
                            || parent.start_line > symbol.start_line
                            || parent.end_line < symbol.end_line
                            || NAMESPACE_KINDS.contains(&parent.kind)
                    })
            })
            .collect();
    }
    outline.into_iter().zip(keep).filter_map(|(symbol, keep)| keep.then_some(symbol)).collect()
}

/// The innermost symbol of an interface outline whose declaration, decorators included, spans `line`.
pub fn enclosing(outline: &[LspSymbol], line: i64) -> Option<&LspSymbol> {
    outline.iter().filter(|s| s.start_line <= line && line <= s.end_line).min_by_key(|s| s.end_line - s.start_line)
}

/// The symbols of an interface outline `changes` falls inside, each the innermost around some
/// change, in outline order, with the changed lines it holds. A symbol is added when the patch adds
/// every line of it.
pub fn changed_symbols(outline: &[LspSymbol], changes: &NewSideChanges) -> Vec<Changed> {
    // Sweep symbol boundaries instead of scanning every symbol for every changed line.
    // The active set chooses the shortest enclosing span, with outline order breaking ties.
    let mut events = Vec::with_capacity(outline.len() * 2);
    for (i, symbol) in outline.iter().enumerate() {
        if symbol.end_line < symbol.start_line {
            continue;
        }
        events.push((symbol.start_line, true, i));
        events.push((symbol.end_line.saturating_add(1), false, i));
    }
    events.sort_unstable();
    let mut active = std::collections::BTreeSet::new();
    let mut hits = vec![0usize; outline.len()];
    let mut removed = changes.removed_at.clone();
    removed.sort_unstable();
    let mut at = 0;
    while at < events.len() {
        let start = events[at].0;
        while at < events.len() && events[at].0 == start {
            let (_, add, i) = events[at];
            let key = (outline[i].end_line - outline[i].start_line, i);
            if add {
                active.insert(key);
            } else {
                active.remove(&key);
            }
            at += 1;
        }
        let Some(&(end, _, _)) = events.get(at) else { break };
        let Some(&(_, i)) = active.first() else { continue };
        let first = changes.added.partition_point(|&(_, hi)| hi < start);
        for &(lo, hi) in &changes.added[first..] {
            if lo >= end {
                break;
            }
            hits[i] += (hi.min(end - 1) - lo.max(start) + 1).max(0) as usize;
        }
        hits[i] += removed.partition_point(|&line| line < end) - removed.partition_point(|&line| line < start);
    }
    outline
        .iter()
        .zip(hits)
        .filter(|(_, hits)| *hits > 0)
        .map(|(s, lines)| {
            let status = if changes.adds_all(s.start_line, s.end_line) {
                GraphNodeStatus::Added
            } else {
                GraphNodeStatus::Modified
            };
            Changed { symbol: s.clone(), status, lines }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbol(name: &str, kind: i64, line: i64, end_line: i64) -> LspSymbol {
        LspSymbol {
            name: name.into(),
            kind,
            container: None,
            path: "a.py".into(),
            line,
            start_line: line,
            end_line,
            col: 0,
        }
    }

    /// class Foo (1-10) holding method bar (2-4) and field x (5); function baz (12-14) holding the
    /// local function k (13); variable v (16); class C (18-20) holding a callback over the same
    /// lines; constant K (22).
    fn outline() -> Vec<LspSymbol> {
        interface(vec![
            symbol("Foo", 5, 1, 10),
            symbol("bar", 6, 2, 4),
            symbol("x", 8, 5, 5),
            symbol("baz", 12, 12, 14),
            symbol("k", 12, 13, 13),
            symbol("v", 13, 16, 16),
            symbol("C", 5, 18, 20),
            symbol("callback", 12, 18, 20),
            symbol("K", 14, 22, 22),
        ])
    }

    fn names(changed: &[Changed]) -> Vec<(&str, GraphNodeStatus)> {
        changed.iter().map(|c| (c.symbol.name.as_str(), c.status)).collect()
    }

    #[test]
    fn maps_each_change_to_the_innermost_interface_symbol_around_it() {
        let changes =
            NewSideChanges { added: vec![(3, 3), (5, 5), (22, 22)], removed_at: vec![13, 19], ..Default::default() };
        assert_eq!(
            names(&changed_symbols(&outline(), &changes)),
            [
                ("Foo", GraphNodeStatus::Modified),
                ("bar", GraphNodeStatus::Modified),
                ("baz", GraphNodeStatus::Modified),
                ("C", GraphNodeStatus::Modified)
            ]
        );
    }

    #[test]
    fn a_wholly_added_symbol_is_added_and_a_change_outside_every_interface_symbol_is_none() {
        let changes = NewSideChanges { added: vec![(12, 16)], ..Default::default() };
        assert_eq!(names(&changed_symbols(&outline(), &changes)), [("baz", GraphNodeStatus::Added)]);
    }

    #[test]
    fn a_change_to_a_decorator_alone_modifies_the_symbol_it_decorates() {
        let route = LspSymbol { start_line: 3, ..symbol("route", 12, 4, 6) };
        let changes = NewSideChanges { added: vec![(3, 3)], ..Default::default() };
        assert_eq!(names(&changed_symbols(&interface(vec![route]), &changes)), [("route", GraphNodeStatus::Modified)]);
    }
}

#[cfg(test)]
mod indexed_tests {
    use super::*;
    fn symbol(i: usize, start: i64, end: i64) -> LspSymbol {
        LspSymbol {
            name: i.to_string(),
            kind: 12,
            container: None,
            path: "a.rs".into(),
            line: start,
            start_line: start,
            end_line: end,
            col: 0,
        }
    }
    #[test]
    fn indexed_hits_match_line_by_line_reference_for_nested_and_overlapping_spans() {
        for seed in 1..40 {
            let outline: Vec<_> = (0..80)
                .map(|i| {
                    let start = ((i * 17 + seed * 13) % 120) as i64 + 1;
                    symbol(i, start, start + ((i * 7 + seed) % 30) as i64)
                })
                .collect();
            let changes = NewSideChanges {
                added: vec![(3, 15), (30, 55), (80, 110)],
                removed_at: vec![1, 15, 15, 50, 125],
                ..Default::default()
            };
            let mut expected = vec![0; outline.len()];
            for line in changes.added.iter().flat_map(|&(lo, hi)| lo..=hi).chain(changes.removed_at.iter().copied()) {
                if let Some(s) = enclosing(&outline, line) {
                    expected[outline.iter().position(|x| std::ptr::eq(x, s)).unwrap()] += 1;
                }
            }
            let got = changed_symbols(&outline, &changes);
            let got: Vec<_> = got.iter().map(|c| (c.symbol.name.parse::<usize>().unwrap(), c.lines)).collect();
            let expected: Vec<_> = expected.into_iter().enumerate().filter(|(_, hits)| *hits > 0).collect();
            assert_eq!(got, expected);
        }
    }
    #[test]
    fn very_large_change_counts_ranges_without_visiting_each_line() {
        let outline = vec![symbol(0, 1, 1_000_000_000)];
        let changes = NewSideChanges { added: vec![(1, 1_000_000_000)], ..Default::default() };
        let start = std::time::Instant::now();
        assert_eq!(changed_symbols(&outline, &changes)[0].lines, 1_000_000_000);
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
    }
}

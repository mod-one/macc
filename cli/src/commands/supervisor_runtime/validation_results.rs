//! Compare complete named test runs. Unknown, mixed or truncated output cannot certify a repair.
use super::io;
use macc_core::Result;
use std::{collections::BTreeMap, fs::File, io::Read, path::Path};

pub(super) enum Comparison {
    Preexisting,
    Regression(Vec<String>),
    Indeterminate,
}

// Accept flat TAP 13 with an explicit plan and unique, numbered, named results.
// Directives, nested suites and diagnostics outside comments need a richer parser.
fn named_results(report: &str) -> Option<BTreeMap<&str, bool>> {
    let mut lines = report
        .lines()
        .filter(|s| !s.is_empty() && !s.starts_with('#'));
    if lines.next()? != "TAP version 13" {
        return None;
    }
    let mut plan = None;
    let mut tests = BTreeMap::new();
    for line in lines {
        if let Some(count) = line.strip_prefix("1..") {
            let count = count.parse::<usize>().ok()?;
            if plan.replace(count).is_some() || (!tests.is_empty() && tests.len() != count) {
                return None;
            }
            continue;
        }
        let (failed, result) = if let Some(result) = line.strip_prefix("not ok ") {
            (true, result)
        } else {
            (false, line.strip_prefix("ok ")?)
        };
        let (number, name) = result.split_once(" - ")?;
        if number.parse::<usize>().ok()? != tests.len() + 1
            || name.trim().is_empty()
            || name.contains('#')
            || tests.insert(name, failed).is_some()
        {
            return None;
        }
    }
    (plan == Some(tests.len()) && tests.values().any(|failed| *failed)).then_some(tests)
}

fn read_report(path: &Path) -> Result<Option<String>> {
    const MAX_BYTES: u64 = 4 * 1024 * 1024;
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|f| f.take(MAX_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|e| io(path, e))?;
    Ok(if bytes.len() as u64 <= MAX_BYTES {
        String::from_utf8(bytes).ok()
    } else {
        None
    })
}

pub(super) fn compare(base: &Path, repair: &Path) -> Result<Comparison> {
    let base = read_report(base)?;
    let repair = read_report(repair)?;
    let (Some(base), Some(repair)) = (
        base.as_deref().and_then(named_results),
        repair.as_deref().and_then(named_results),
    ) else {
        return Ok(Comparison::Indeterminate);
    };
    let added: Vec<_> = repair
        .iter()
        .filter(|(id, failed)| **failed && base.get(**id) != Some(&true))
        .map(|(id, _)| (*id).to_owned())
        .collect();
    Ok(if !added.is_empty() {
        Comparison::Regression(added)
    } else if base.keys().all(|id| repair.contains_key(id)) {
        Comparison::Preexisting
    } else {
        Comparison::Indeterminate
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incomplete_or_ambiguous_runs_have_no_comparable_failure_set() {
        for run in [
            "1..2\nnot ok 1 - first",
            "1..2\nnot ok 1 - repeated\nok 2 - repeated",
            "1..1\nnot ok 1 - unstable # TODO",
            "1..1\nnot ok 1 - old\nBail out!",
            "1..1\nok 1 - passed",
        ] {
            assert!(named_results(&format!("TAP version 13\n{run}")).is_none());
        }
    }
}

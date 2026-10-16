//! Bounded agent transport. Full operator inspection remains a separate operation.
use super::*;
use serde::Serialize;
#[derive(Clone, Serialize)]
pub struct PullPage {
    pub records: Vec<Hit>,
    pub references: Vec<String>,
    pub next_cursor: Option<usize>,
    pub estimated_tokens: usize,
    pub limit_tokens: usize,
}
#[derive(Serialize)]
pub struct ReadPage {
    pub id: String,
    pub checksum: String,
    pub offset: usize,
    pub fragment: String,
    pub next_offset: Option<usize>,
    pub total_chars: usize,
    pub estimated_tokens: usize,
    pub limit_tokens: usize,
}
fn measure<T: Serialize>(page: &T) -> Result<usize> {
    Ok(crate::context::estimate_tokens(
        &serde_json::to_string(page).map_err(|e| error(e.to_string()))?,
    ))
}
impl PullPage {
    fn size(&mut self) -> Result<usize> {
        for _ in 0..4 {
            self.estimated_tokens = measure(self)?;
        }
        Ok(self.estimated_tokens)
    }
}
pub fn pull(root: &Path, request: &Search, cursor: usize) -> Result<PullPage> {
    let limit = super::super::policy::load(root)?
        .context_engine
        .pull_limit_tokens as usize;
    let hits = search(root, request)?;
    if cursor > hits.len() {
        return Err(error("Pull cursor is outside this result set"));
    }
    let mut page = PullPage {
        records: vec![],
        references: vec![],
        next_cursor: None,
        estimated_tokens: 0,
        limit_tokens: limit,
    };
    for (i, hit) in hits.iter().enumerate().skip(cursor) {
        let mut candidate = page.clone();
        candidate.next_cursor = (i + 1 < hits.len()).then_some(i + 1);
        candidate.records.push(hit.clone());
        if candidate.size()? <= limit {
            page = candidate;
            continue;
        }
        candidate = page.clone();
        candidate.next_cursor = (i + 1 < hits.len()).then_some(i + 1);
        candidate.references.push(hit.memory.id.clone());
        if candidate.size()? <= limit {
            page = candidate;
        } else {
            page.next_cursor = Some(i);
            break;
        }
    }
    page.size()?;
    Ok(page)
}
pub fn read(root: &Path, id: &str, access: &Access, offset: usize) -> Result<ReadPage> {
    let hit = get(root, id, access)?;
    let content = serde_json::to_string(&hit).map_err(|e| error(e.to_string()))?;
    let chars = content.chars().collect::<Vec<_>>();
    if offset > chars.len() {
        return Err(error("Memory fragment offset is outside the record"));
    }
    let limit = super::super::policy::load(root)?
        .context_engine
        .pull_limit_tokens as usize;
    let mut page = ReadPage {
        id: id.into(),
        checksum: hit.checksum,
        offset,
        fragment: String::new(),
        next_offset: Some(offset),
        total_chars: chars.len(),
        estimated_tokens: 0,
        limit_tokens: limit,
    };
    let mut low = 0;
    let mut high = chars.len() - offset;
    while low < high {
        let count = (low + high).div_ceil(2);
        page.fragment = chars[offset..offset + count].iter().collect();
        page.next_offset = (offset + count < chars.len()).then_some(offset + count);
        for _ in 0..4 {
            page.estimated_tokens = measure(&page)?;
        }
        if page.estimated_tokens <= limit {
            low = count;
        } else {
            high = count - 1;
        }
    }
    page.fragment = chars[offset..offset + low].iter().collect();
    page.next_offset = (offset + low < chars.len()).then_some(offset + low);
    for _ in 0..4 {
        page.estimated_tokens = measure(&page)?;
    }
    if page.estimated_tokens > limit || low == 0 && offset < chars.len() {
        return Err(error("Pull budget cannot hold fragment framing"));
    }
    Ok(page)
}

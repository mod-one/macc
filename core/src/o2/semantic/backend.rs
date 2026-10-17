//! Local embedding port, independent of performer/model providers and shell commands.
use super::*;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Descriptor {
    pub id: String,
    pub recipe: String,
    pub local_only: bool,
    pub description: String,
}
/// Numeric limits, deliberately separate from memory policy and performer settings.
pub struct EmbeddingLimits {
    pub dimensions: u64,
    pub vocabulary: u64,
    pub max_ms: u64,
    pub max_bytes: u64,
}
pub trait EmbeddingSpace: Send + Sync {
    /// One finite cosine score per input document, in the original document order.
    fn similarities(&self, query: &str) -> Result<Vec<f64>>;
    fn bytes(&self) -> usize;
}
pub trait EmbeddingBackend: Send + Sync {
    fn descriptor(&self) -> Descriptor;
    fn fit(
        &self,
        documents: &[String],
        policy: &EmbeddingLimits,
    ) -> Result<Box<dyn EmbeddingSpace>>;
}
pub struct Registration {
    pub create: fn() -> Box<dyn EmbeddingBackend>,
}
inventory::collect!(Registration);
pub(super) fn backend(id: &str) -> Result<Box<dyn EmbeddingBackend>> {
    let mut values: Vec<Box<dyn EmbeddingBackend>> = vec![Box::new(super::lsa::Lsa)];
    values.extend(
        inventory::iter::<Registration>
            .into_iter()
            .map(|r| (r.create)()),
    );
    let mut ids = std::collections::BTreeSet::new();
    for value in &values {
        let d = value.descriptor();
        if d.id.is_empty()
            || d.id.len() > 64
            || !d
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
            || !ids.insert(d.id.clone())
            || d.recipe.is_empty()
            || d.recipe.len() > 128
            || d.description.len() > 512
        {
            return Err(error("Invalid embedding backend registration"));
        }
    }
    let backend = values
        .into_iter()
        .find(|b| b.descriptor().id == id)
        .ok_or_else(|| error("Unknown local embedding backend"))?;
    if !backend.descriptor().local_only {
        return Err(error("Remote embedding backends are forbidden for P3-C"));
    }
    Ok(backend)
}
pub(super) fn fit(
    records: &[Memory],
    p: &MemoryPolicy,
) -> Result<(Descriptor, Box<dyn EmbeddingSpace>)> {
    if records.len() > p.semantic_candidates as usize {
        return Err(error("Semantic candidate budget exceeded; FTS retained"));
    }
    let mut bytes = 0usize;
    let documents = records
        .iter()
        .map(|m| {
            let text = format!("{}\n{}", m.proposal.title, m.proposal.body);
            bytes += text.len();
            super::super::paths::safe_text(&text)?;
            Ok(text)
        })
        .collect::<Result<Vec<_>>>()?;
    if bytes > p.semantic_max_bytes as usize {
        return Err(error("Semantic input byte budget exceeded"));
    }
    let backend = backend(&p.semantic_backend)?;
    let started = std::time::Instant::now();
    let limits = EmbeddingLimits {
        dimensions: p.semantic_dimensions,
        vocabulary: p.semantic_vocabulary,
        max_ms: p.semantic_max_ms,
        max_bytes: p.semantic_max_bytes,
    };
    let model = backend.fit(&documents, &limits)?;
    if model.bytes() > p.semantic_max_bytes as usize
        || started.elapsed().as_millis() > p.semantic_max_ms as u128
    {
        return Err(error(
            "Embedding backend exceeded its local resource budget",
        ));
    }
    Ok((backend.descriptor(), model))
}
pub(super) fn scores(
    model: &dyn EmbeddingSpace,
    query: &str,
    records: &[Memory],
    p: &MemoryPolicy,
) -> Result<BTreeMap<String, f64>> {
    if query.len() > 512 {
        return Err(error("Semantic query exceeds bound"));
    }
    super::super::paths::safe_text(query)?;
    let start = std::time::Instant::now();
    let values = model.similarities(query)?;
    if values.len() != records.len()
        || values
            .iter()
            .any(|v| !v.is_finite() || !(-1.000001..=1.000001).contains(v))
        || start.elapsed().as_millis() > p.semantic_max_ms as u128
    {
        return Err(error("Invalid or over-budget local embedding response"));
    }
    Ok(records
        .iter()
        .zip(values)
        .filter(|(_, score)| *score >= p.semantic_min_similarity)
        .map(|(m, score)| (m.id.clone(), score.clamp(0., 1.)))
        .collect())
}

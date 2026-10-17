//! Corpus-local TF-IDF truncated SVD via deterministic orthogonal power iteration.
//! This is classical LSA, not a pretrained neural or multilingual embedding model.
use super::*;
use backend::{EmbeddingBackend, EmbeddingSpace};
pub(super) struct Lsa;
struct Space {
    vocab: BTreeMap<String, usize>,
    idf: Vec<f64>,
    basis: Vec<Vec<f64>>,
    vectors: Vec<Vec<f64>>,
}
fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|s| {
            s.chars().count() > 1
                && ![
                    "the", "and", "for", "with", "from", "this", "that", "are", "was", "will",
                    "into", "not", "has",
                ]
                .contains(&s.as_str())
        })
        .collect()
}
fn norm(v: &mut [f64]) -> f64 {
    let n = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    if n > 1e-12 {
        for x in v {
            *x /= n;
        }
    }
    n
}
fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}
fn orthogonalize(v: &mut [f64], basis: &[Vec<f64>]) {
    for b in basis {
        let d = dot(v, b);
        for (x, y) in v.iter_mut().zip(b) {
            *x -= d * y;
        }
    }
}
impl EmbeddingBackend for Lsa {
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            id: "local_lsa".into(),
            recipe: "tfidf-lsa-power12-v1".into(),
            local_only: true,
            description:
                "Local corpus TF-IDF/LSA; no pretrained weights, network, subprocess or downloads"
                    .into(),
        }
    }
    fn fit(
        &self,
        documents: &[String],
        p: &backend::EmbeddingLimits,
    ) -> Result<Box<dyn EmbeddingSpace>> {
        let start = std::time::Instant::now();
        if documents.len() < 3 {
            return Err(error("LSA requires at least three eligible documents"));
        }
        let terms = documents.iter().map(|s| tokens(s)).collect::<Vec<_>>();
        let mut frequency = BTreeMap::<String, usize>::new();
        for doc in &terms {
            for term in doc.iter().collect::<std::collections::BTreeSet<_>>() {
                *frequency.entry(term.clone()).or_default() += 1;
            }
        }
        let mut selected = frequency.into_iter().collect::<Vec<_>>();
        selected.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        selected.truncate(p.vocabulary as usize);
        if selected.len() < 3 {
            return Err(error("LSA vocabulary has insufficient variation"));
        }
        let vocab = selected
            .iter()
            .enumerate()
            .map(|(i, (s, _))| (s.clone(), i))
            .collect::<BTreeMap<_, _>>();
        let idf = selected
            .iter()
            .map(|(_, n)| 1. + ((documents.len() + 1) as f64 / (*n + 1) as f64).ln())
            .collect::<Vec<_>>();
        let width = vocab.len();
        let dimensions = (p.dimensions as usize)
            .min(width - 1)
            .min(documents.len() - 1);
        if (documents.len() * width + width * dimensions + documents.len() * dimensions) * 8
            > p.max_bytes as usize
        {
            return Err(error("LSA matrix memory budget exceeded"));
        }
        let encode = |words: Vec<String>| {
            let mut v = vec![0_f64; width];
            for word in words {
                if let Some(i) = vocab.get(&word) {
                    v[*i] += 1.;
                }
            }
            for (x, idf) in v.iter_mut().zip(&idf) {
                *x = if *x > 0. { (1. + (*x).ln()) * idf } else { 0. };
            }
            norm(&mut v);
            v
        };
        let matrix = terms.into_iter().map(encode).collect::<Vec<_>>();
        let mut basis: Vec<Vec<f64>> = vec![];
        for axis in 0..dimensions {
            let mut seed = axis as u64 + 1;
            let mut v = (0..width)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    (seed % 2001) as f64 / 1000. - 1.
                })
                .collect::<Vec<_>>();
            orthogonalize(&mut v, &basis);
            norm(&mut v);
            for _ in 0..12 {
                if start.elapsed().as_millis() >= p.max_ms as u128 {
                    return Err(error("LSA training timed out; FTS retained"));
                }
                let mut next = vec![0_f64; width];
                for row in &matrix {
                    let weight = dot(row, &v);
                    for (x, y) in next.iter_mut().zip(row) {
                        *x += weight * y;
                    }
                }
                orthogonalize(&mut next, &basis);
                if norm(&mut next) < 1e-10 {
                    break;
                }
                v = next;
            }
            orthogonalize(&mut v, &basis);
            if norm(&mut v) > 1e-10 {
                basis.push(v);
            }
        }
        if basis.is_empty() {
            return Err(error("LSA decomposition is degenerate"));
        }
        let vectors = matrix
            .iter()
            .map(|row| {
                let mut v = basis.iter().map(|b| dot(row, b)).collect::<Vec<_>>();
                norm(&mut v);
                v
            })
            .collect();
        Ok(Box::new(Space {
            vocab,
            idf,
            basis,
            vectors,
        }))
    }
}
impl EmbeddingSpace for Space {
    fn bytes(&self) -> usize {
        (self.basis.iter().map(Vec::len).sum::<usize>()
            + self.vectors.iter().map(Vec::len).sum::<usize>()
            + self.idf.len())
            * 8
            + self.vocab.keys().map(String::len).sum::<usize>()
    }
    fn similarities(&self, query: &str) -> Result<Vec<f64>> {
        let mut row = vec![0_f64; self.vocab.len()];
        for term in tokens(query) {
            if let Some(i) = self.vocab.get(&term) {
                row[*i] += 1.;
            }
        }
        for (x, idf) in row.iter_mut().zip(&self.idf) {
            *x = if *x > 0. { (1. + (*x).ln()) * idf } else { 0. };
        }
        norm(&mut row);
        let mut q = self.basis.iter().map(|b| dot(&row, b)).collect::<Vec<_>>();
        norm(&mut q);
        Ok(self
            .vectors
            .iter()
            .map(|v| dot(&q, v).clamp(-1., 1.))
            .collect())
    }
}

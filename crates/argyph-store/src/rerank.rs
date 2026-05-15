use crate::search::SearchHit;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FocusContext {
    pub file: Option<String>,
    pub symbol: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weights {
    pub base: f32,
    pub recency: f32,
    pub focus_call: f32,
    pub focus_module: f32,
    pub size_penalty: f32,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            base: 1.0,
            recency: 0.15,
            focus_call: 0.30,
            focus_module: 0.15,
            size_penalty: -0.10,
        }
    }
}

pub fn rerank_hits(hits: &mut [SearchHit], focus: &FocusContext, weights: Weights) {
    for hit in hits.iter_mut() {
        let recency_signal = 0.0;
        let call_distance = 0.0;
        let module_match = focus
            .file
            .as_deref()
            .map(|file| same_module(file, &hit.file))
            .unwrap_or(0.0);
        let size_penalty = huge_chunk_penalty(hit.chunk_text.len());

        hit.score = weights.base.mul_add(
            hit.score,
            weights.recency * recency_signal
                + weights.focus_call * call_distance
                + weights.focus_module * module_match
                + weights.size_penalty * size_penalty,
        );
    }

    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.chunk_id.cmp(&b.chunk_id))
    });
}

fn same_module(focus_file: &str, hit_file: &str) -> f32 {
    let focus_dir = focus_file
        .rsplit_once('/')
        .map(|(dir, _)| dir)
        .unwrap_or("");
    let hit_dir = hit_file.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
    if !focus_dir.is_empty() && focus_dir == hit_dir {
        1.0
    } else {
        0.0
    }
}

fn huge_chunk_penalty(bytes: usize) -> f32 {
    if bytes >= 16 * 1024 {
        1.0
    } else if bytes >= 8 * 1024 {
        0.5
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use crate::search::HitSource;

    use super::*;

    fn hit(id: &str, file: &str, text_len: usize, score: f32) -> SearchHit {
        SearchHit {
            chunk_id: id.into(),
            chunk_text: "x".repeat(text_len),
            file: file.into(),
            byte_range: (0, text_len as u32),
            line_range: (1, 1),
            score,
            source: HitSource::Hybrid,
        }
    }

    #[test]
    fn module_focus_boosts_same_directory_hit() {
        let mut hits = vec![
            hit("a", "src/auth/login.rs", 20, 0.5),
            hit("b", "src/db/query.rs", 20, 0.5),
        ];
        rerank_hits(
            &mut hits,
            &FocusContext {
                file: Some("src/auth/mod.rs".into()),
                symbol: None,
            },
            Weights::default(),
        );
        assert_eq!(hits[0].chunk_id, "a");
        assert!(hits[0].score > hits[1].score);
    }

    #[test]
    fn huge_chunks_are_penalized_when_base_scores_tie() {
        let mut hits = vec![
            hit("large", "src/a.rs", 20_000, 0.5),
            hit("small", "src/b.rs", 20, 0.5),
        ];
        rerank_hits(&mut hits, &FocusContext::default(), Weights::default());
        assert_eq!(hits[0].chunk_id, "small");
    }
}

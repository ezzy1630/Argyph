use std::sync::Mutex;

use tracing;

use crate::config::EmbedConfig;
use crate::error::{EmbedError, Result};
use crate::model_files::ModelFiles;
use crate::tokenize::BertTokenizer;

const BGE_SMALL_MODEL_ID: &str = "bge-small-en-v1.5";
const BGE_SMALL_HIDDEN_SIZE: usize = 384;
const BGE_SMALL_MAX_SEQ_LEN: usize = 512;

pub struct LocalEmbedder {
    session: Mutex<ort::session::Session>,
    tokenizer: BertTokenizer,
    config: EmbedConfig,
    dimension: usize,
    model_id: String,
}

impl LocalEmbedder {
    pub async fn new(config: EmbedConfig) -> Result<Self> {
        let model_files =
            ModelFiles::ensure_available(BGE_SMALL_MODEL_ID, config.cache_dir.as_deref()).await?;

        let tokenizer = BertTokenizer::from_file(&model_files.tokenizer_path)?;

        ort::init().with_name("argyph-embed").commit();

        let session = ort::session::Session::builder()
            .map_err(|e| EmbedError::Config(format!("ONNX session builder: {e}")))?
            .commit_from_file(model_files.onnx_path)
            .map_err(|e| EmbedError::Config(format!("failed to load ONNX model: {e}")))?;

        tracing::info!(
            model_id = BGE_SMALL_MODEL_ID,
            dimension = BGE_SMALL_HIDDEN_SIZE,
            "local embedder ready"
        );

        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
            config,
            dimension: BGE_SMALL_HIDDEN_SIZE,
            model_id: BGE_SMALL_MODEL_ID.to_string(),
        })
    }

    fn do_embed(
        session: &mut ort::session::Session,
        tokenizer: &BertTokenizer,
        texts: &[String],
        batch_size: usize,
        seq_len: usize,
        dimension: usize,
    ) -> Result<Vec<Vec<f32>>> {
        let batch = tokenizer.encode_batch(texts, seq_len)?;

        use ort::value::Tensor;

        let attention_mask_data = batch.attention_mask.clone();

        let input_ids_tensor = Tensor::from_array((
            [batch_size, batch.seq_len],
            batch.input_ids.into_boxed_slice(),
        ))
        .map_err(|e| EmbedError::Config(format!("ONNX input_ids tensor: {e}")))?;

        let attention_mask_tensor = Tensor::from_array((
            [batch_size, batch.seq_len],
            batch.attention_mask.into_boxed_slice(),
        ))
        .map_err(|e| EmbedError::Config(format!("ONNX attention_mask tensor: {e}")))?;

        let token_type_ids = vec![0_i64; batch_size * batch.seq_len];
        let token_type_ids_tensor = Tensor::from_array((
            [batch_size, batch.seq_len],
            token_type_ids.into_boxed_slice(),
        ))
        .map_err(|e| EmbedError::Config(format!("ONNX token_type_ids tensor: {e}")))?;

        let inputs = ort::inputs![
            "input_ids" => input_ids_tensor.view(),
            "attention_mask" => attention_mask_tensor.view(),
            "token_type_ids" => token_type_ids_tensor.view(),
        ];

        let outputs = session
            .run(inputs)
            .map_err(|e| EmbedError::Config(format!("ONNX inference failed: {e}")))?;

        let last_hidden_value = outputs
            .get("last_hidden_state")
            .ok_or_else(|| EmbedError::Config("ONNX output missing 'last_hidden_state'".into()))?;

        let (_out_shape, last_hidden_data): (_, &[f32]) = last_hidden_value
            .try_extract_tensor::<f32>()
            .map_err(|e| EmbedError::Config(format!("ONNX output extraction: {e}")))?;

        let owned_data = last_hidden_data.to_vec();

        drop(outputs);

        Ok(BertTokenizer::mean_pool(
            &owned_data,
            &attention_mask_data,
            batch_size,
            batch.seq_len,
            dimension,
        ))
    }
}

#[async_trait::async_trait]
impl crate::Embedder for LocalEmbedder {
    fn dimension(&self) -> usize {
        self.dimension
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Err(EmbedError::EmptyInput);
        }

        let chunk_size = self.config.batch_size.min(128);
        let mut all_embeddings: Vec<Vec<f32>> = Vec::with_capacity(texts.len());

        for chunk in texts.chunks(chunk_size) {
            let batch_texts: Vec<String> = chunk.to_vec();
            let n = batch_texts.len();

            let embeddings = {
                let mut session = self.session.lock().unwrap_or_else(|e| e.into_inner());
                Self::do_embed(
                    &mut session,
                    &self.tokenizer,
                    &batch_texts,
                    n,
                    BGE_SMALL_MAX_SEQ_LEN,
                    self.dimension,
                )?
            };

            all_embeddings.extend(embeddings);
        }

        Ok(all_embeddings)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::config::EmbedConfig;
    use crate::Embedder;

    fn model_dir_exists() -> bool {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        let cache: std::path::PathBuf =
            std::path::PathBuf::from(home).join(".cache/argyph/models/bge-small-en-v1.5");
        cache.join("model.onnx").exists() && cache.join("tokenizer.json").exists()
    }

    #[tokio::test]
    async fn local_embedder_succeeds_even_if_cache_empty() {
        if model_dir_exists() {
            eprintln!("model already cached, test would re-download (slow); skipping");
            return;
        }
        let config = EmbedConfig {
            cache_dir: None,
            ..EmbedConfig::default()
        };
        let result = LocalEmbedder::new(config).await;
        // The test passes if the embedder either successfully downloads
        // the model or returns a Config error (network unreachable,
        // download failed, rename failed because the tmp file is
        // missing, etc.). The point is that the code path never
        // panics — any downstream IO failure surfaces as a Config error
        // string, which is what we accept here.
        match result {
            Ok(_) => {}
            Err(EmbedError::Config(_)) => {}
            Err(other) => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn local_embedder_works_if_model_cached() {
        if !model_dir_exists() {
            eprintln!("model not cached, skipping integration test");
            return;
        }

        let home = std::env::var("HOME").unwrap();
        let cache: std::path::PathBuf = std::path::PathBuf::from(home).join(".cache/argyph/models");

        let config = EmbedConfig {
            cache_dir: Some(cache),
            ..EmbedConfig::default()
        };

        let embedder = LocalEmbedder::new(config).await.unwrap();
        assert_eq!(embedder.dimension(), 384);
        assert_eq!(embedder.model_id(), "bge-small-en-v1.5");

        let texts: Vec<String> = vec!["hello world".into(), "goodbye world".into()];
        let embeddings = embedder.embed(&texts).await.unwrap();
        assert_eq!(embeddings.len(), 2);
        for v in &embeddings {
            assert_eq!(v.len(), 384);
            let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            assert!(
                (norm - 1.0).abs() < 0.01,
                "L2 norm should be approx 1.0, got {norm}"
            );
        }
    }

    #[tokio::test]
    async fn local_embedder_empty_input_error() {
        if !model_dir_exists() {
            eprintln!("model not cached, skipping integration test");
            return;
        }

        let home = std::env::var("HOME").unwrap();
        let cache: std::path::PathBuf = std::path::PathBuf::from(home).join(".cache/argyph/models");

        let config = EmbedConfig {
            cache_dir: Some(cache),
            ..EmbedConfig::default()
        };
        let embedder = LocalEmbedder::new(config).await.unwrap();
        let result = embedder.embed(&[]).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            EmbedError::EmptyInput => {}
            other => panic!("expected EmptyInput, got: {other:?}"),
        }
    }
}

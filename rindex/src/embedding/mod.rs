use anyhow::{Context, Result};
use std::path::Path;
use tokenizers::Tokenizer;

pub struct Embedder {
    /// token_id -> embedding_vec, shape [vocab_size, dim]
    embeddings: Vec<Vec<f32>>,
    tokenizer: Tokenizer,
    dim: usize,
}

impl Embedder {
    /// Load a static token embedding model from a directory.
    ///
    /// Expected files in `model_dir`:
    ///   - `token_embeddings.safetensors`  — [vocab_size, dim] f32 matrix
    ///   - `tokenizer.json`                — HuggingFace tokenizer
    ///   - `config.json`                   — {"vocab_size", "dim", "model_type"}
    ///
    /// The `model_id` parameter is used as a subdirectory name under `cache_dir`.
    /// The `_hf_endpoint` parameter is kept for API compatibility but unused
    /// (distilled models are local files, not downloaded from HuggingFace).
    pub fn load(cache_dir: &Path, model_id: &str, _hf_endpoint: Option<&str>) -> Result<Self> {
        let model_dir = cache_dir.join(model_id);

        if !model_dir.exists() {
            anyhow::bail!(
                "Static embedding model not found at: {}\n\
                 Run 'python scripts/distill.py --output {}' first, then copy the output there.",
                model_dir.display(),
                model_dir.display(),
            );
        }

        // 1. Load tokenizer
        let tokenizer_path = model_dir.join("tokenizer.json");
        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| anyhow::anyhow!("Failed to load tokenizer: {}", e))?;

        // 2. Load config
        let config_path = model_dir.join("config.json");
        let config_raw = std::fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read {}", config_path.display()))?;
        let config: serde_json::Value = serde_json::from_str(&config_raw)
            .context("Failed to parse config.json")?;

        let vocab_size = config["vocab_size"].as_u64()
            .context("config.json missing vocab_size")? as usize;
        let dim = config["dim"].as_u64()
            .context("config.json missing dim")? as usize;

        // 3. Load token embeddings (safetensors)
        let emb_path = model_dir.join("token_embeddings.safetensors");
        let data = std::fs::read(&emb_path)
            .with_context(|| format!("Failed to read {}", emb_path.display()))?;

        let safetensors = safetensors::SafeTensors::deserialize(&data)
            .context("Failed to parse safetensors file")?;

        let view = safetensors.tensor("token_embeddings")
            .context("safetensors missing 'token_embeddings' tensor")?;

        let shape = view.shape();
        if shape.len() != 2 || shape[0] != vocab_size || shape[1] != dim {
            anyhow::bail!(
                "Expected tensor shape [{}, {}], got {:?}",
                vocab_size, dim, shape
            );
        }

        // safetensors stores raw bytes; interpret as f32 little-endian
        let raw: &[u8] = view.data();
        let floats: Vec<f32> = raw
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();

        if floats.len() != vocab_size * dim {
            anyhow::bail!(
                "Data size mismatch: expected {} floats ({}×{}), got {}",
                vocab_size * dim, vocab_size, dim, floats.len()
            );
        }

        let embeddings: Vec<Vec<f32>> = floats.chunks_exact(dim).map(|c| c.to_vec()).collect();

        tracing::info!(
            "Loaded static embedding model: {} tokens × {} dim ({:.0} MB)",
            vocab_size,
            dim,
            data.len() as f64 / (1024.0 * 1024.0),
        );

        Ok(Self { embeddings, tokenizer, dim })
    }

    /// Embed a single text into a fixed-dimensional vector.
    ///
    /// Algorithm:
    ///   1. Tokenize the text
    ///   2. Look up each token's static embedding from the table
    ///   3. Mean pool (average all token vectors)
    ///   4. L2 normalize
    pub fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let encoding = self.tokenizer
            .encode(text, true) // add_special_tokens = true
            .map_err(|e| anyhow::anyhow!("Tokenization failed: {}", e))?;

        let ids = encoding.get_ids();

        if ids.is_empty() {
            return Ok(vec![0.0_f32; self.dim]);
        }

        // Mean pool
        let mut pooled = vec![0.0_f32; self.dim];
        let mut count: usize = 0;

        for &tok_id in ids {
            let idx = tok_id as usize;
            if idx < self.embeddings.len() {
                let emb = &self.embeddings[idx];
                for i in 0..self.dim {
                    pooled[i] += emb[i];
                }
                count += 1;
            }
        }

        if count == 0 {
            return Ok(vec![0.0_f32; self.dim]);
        }

        for v in &mut pooled {
            *v /= count as f32;
        }

        // L2 normalize
        let norm: f32 = pooled.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in &mut pooled {
                *v /= norm;
            }
        }

        Ok(pooled)
    }

    /// Embed multiple texts. Each text is processed independently.
    pub fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        texts.iter().map(|t| self.embed(t)).collect()
    }
}

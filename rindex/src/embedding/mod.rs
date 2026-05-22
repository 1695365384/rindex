use anyhow::{Context, Result};
use candle_core::{Device, Tensor};
use candle_transformers::models::bert::{BertModel, Config, DTYPE};
use hf_hub::api::sync::Api;
use std::path::Path;
use tokenizers::Tokenizer;

pub struct Embedder {
    model: BertModel,
    tokenizer: Tokenizer,
    device: Device,
}

impl Embedder {
    #[allow(unused_variables)]
    pub fn load(cache_dir: &Path, model_id: &str) -> Result<Self> {
        let device = Device::Cpu;

        let api = Api::new()?.repo(hf_hub::Repo::with_revision(
            model_id.to_string(),
            hf_hub::RepoType::Model,
            "main".to_string(),
        ));

        let model_path = api.get("model.safetensors")?;
        let config_path = api.get("config.json")?;
        let tokenizer_path = api.get("tokenizer.json")?;

        let config = std::fs::read_to_string(&config_path)
            .context("Failed to read config.json")?;
        let config: Config = serde_json::from_str(&config)
            .context("Failed to parse BERT config")?;

        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| anyhow::anyhow!("Failed to load tokenizer: {}", e))?;

        let vb = unsafe {
            candle_nn::VarBuilder::from_mmaped_safetensors(
                &[model_path],
                DTYPE,
                &device,
            )?
        };

        let model = BertModel::load(vb, &config)?;

        Ok(Self { model, tokenizer, device })
    }

    pub fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let tokens = self.tokenizer
            .encode(text, true)
            .map_err(|e| anyhow::anyhow!("Failed to tokenize text: {}", e))?;

        let token_ids = tokens.get_ids();
        let token_type_ids = vec![0u32; token_ids.len()];
        let attention_mask = vec![1u32; token_ids.len()];

        let token_ids = Tensor::new(token_ids, &self.device)?.unsqueeze(0)?;
        let token_type_ids = Tensor::new(&token_type_ids[..], &self.device)?.unsqueeze(0)?;
        let attention_mask = Tensor::new(&attention_mask[..], &self.device)?.unsqueeze(0)?;

        let output = self.model.forward(&token_ids, &token_type_ids, Some(&attention_mask))?;

        // Use mean pooling
        let (_n, _seq_len, _hidden) = output.dims3()?;
        let embedding = output
            .mean(1)?
            .squeeze(0)?
            .to_vec1::<f32>()?;

        // L2 normalize
        let norm = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        let normalized: Vec<f32> = embedding.into_iter().map(|x| x / norm).collect();

        Ok(normalized)
    }

    pub fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        texts.iter().map(|t| self.embed(t)).collect()
    }
}

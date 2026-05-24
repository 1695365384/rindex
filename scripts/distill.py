"""
Distill C2LLM-0.5B into a static token embedding table.

Uses the Model2Vec approach:
  1. Forward-pass each token in vocabulary through the teacher model
  2. Collect the last hidden state for each token
  3. Apply PCA whitening to reduce dimensions
  4. Save as safetensors (+ tokenizer.json + config.json)

Usage:
  python scripts/distill.py \
    --teacher codefuse-ai/C2LLM-0.5B \
    --output ./rindex-models/c2llm-static-256/ \
    --dim 256

Output (3 files, ~156MB for 256-dim, ~233MB for 384-dim):
  token_embeddings.safetensors   # [vocab_size, dim] f32
  tokenizer.json                 # Qwen BPE tokenizer
  config.json                    # {vocab_size, dim, model_type: "static"}

Requirements:
  pip install torch transformers safetensors scikit-learn tqdm
"""

import argparse
import json
import os
import shutil
import sys

import numpy as np
import torch
from safetensors.numpy import save_file
from sklearn.decomposition import PCA
from tqdm import tqdm
from transformers import AutoModel, AutoTokenizer


def parse_args():
    p = argparse.ArgumentParser(description="Distill C2LLM into static token embeddings")
    p.add_argument("--teacher", default="codefuse-ai/C2LLM-0.5B",
                   help="Teacher model ID on HuggingFace")
    p.add_argument("--output", required=True,
                   help="Output directory for model files")
    p.add_argument("--dim", type=int, default=256,
                   help="Target embedding dimension (default: 256)")
    p.add_argument("--device", default="cpu",
                   help="Device: cpu or cuda")
    p.add_argument("--batch-size", type=int, default=64,
                   help="Tokens per batch during forward pass")
    p.add_argument("--hf-mirror", action="store_true",
                   help="Use hf-mirror.com for downloads")
    return p.parse_args()


def load_teacher(model_id: str, device: str, use_mirror: bool):
    """Load C2LLM teacher model and tokenizer."""
    kwargs = {}
    if use_mirror:
        os.environ["HF_ENDPOINT"] = "https://hf-mirror.com"

    print(f"Loading teacher model: {model_id} ...")
    tokenizer = AutoTokenizer.from_pretrained(model_id, trust_remote_code=True)
    model = AutoModel.from_pretrained(
        model_id,
        trust_remote_code=True,
        torch_dtype=torch.float32,
        attn_implementation="eager",
    ).to(device)
    model.eval()
    print(f"  vocab_size={tokenizer.vocab_size}, hidden_size={model.config.hidden_size}")
    return model, tokenizer


def distill_vocab(model, tokenizer, device: str, batch_size: int):
    """Forward-pass each token through the teacher, collect hidden states."""
    vocab_size = tokenizer.vocab_size

    # Reserve special tokens at indices 0-99 (BOS, EOS, PAD, etc.)
    # We skip UNK (0) and control tokens — they get zeroed or random init
    start_idx = 0
    end_idx = vocab_size

    hidden_size = model.config.hidden_size
    embeddings = np.zeros((vocab_size, hidden_size), dtype=np.float32)

    print(f"Distilling {vocab_size} tokens (batch_size={batch_size})...")

    for batch_start in tqdm(range(start_idx, end_idx, batch_size)):
        batch_end = min(batch_start + batch_size, end_idx)
        batch_ids = list(range(batch_start, batch_end))

        # Forward each token individually through the model
        # C2LLM (Qwen2.5-Coder) is decoder-only, so we pass single token + EOS
        for token_id in batch_ids:
            input_ids = torch.tensor([[token_id]], device=device)
            attention_mask = torch.ones_like(input_ids)
            with torch.no_grad():
                # C2LLMForEmbedding.forward returns {"sentence_embedding": ...}
                # Use internal plm_model to get hidden states directly
                plm = model.plm_model
                outputs = plm(input_ids=input_ids, attention_mask=attention_mask, output_hidden_states=True)
                last_hidden = outputs.hidden_states[-1]  # [1, 1, hidden]
                vec = last_hidden[0, 0, :].cpu().numpy()
                embeddings[token_id] = vec.astype(np.float32)

    return embeddings


def apply_pca(embeddings: np.ndarray, target_dim: int):
    """PCA whitening to reduce dimension."""
    print(f"Applying PCA: {embeddings.shape[1]} → {target_dim} ...")
    pca = PCA(n_components=target_dim, whiten=True, random_state=42)
    reduced = pca.fit_transform(embeddings)
    # L2 normalize each vector
    norms = np.linalg.norm(reduced, axis=1, keepdims=True)
    norms[norms == 0] = 1.0
    reduced = reduced / norms
    return reduced.astype(np.float32)


def save_model(embeddings: np.ndarray, tokenizer, output_dir: str):
    """Save as safetensors + tokenizer + config."""
    os.makedirs(output_dir, exist_ok=True)

    # Save embeddings
    emb_path = os.path.join(output_dir, "token_embeddings.safetensors")
    print(f"Saving embeddings to {emb_path} ...")
    save_file({"token_embeddings": embeddings}, emb_path)

    # Save tokenizer.json directly
    tok_path = os.path.join(output_dir, "tokenizer.json")
    print(f"Saving tokenizer to {tok_path} ...")
    # Qwen2TokenizerFast 没有 .save()/.save_pretrained()，直接序列化 to_json
    with open(tok_path, "w", encoding="utf-8") as f:
        f.write(tokenizer.to_json())

    # Save config
    cfg_path = os.path.join(output_dir, "config.json")
    print(f"Saving config to {cfg_path} ...")
    with open(cfg_path, "w") as f:
        json.dump({
            "vocab_size": embeddings.shape[0],
            "dim": embeddings.shape[1],
            "model_type": "static",
            "teacher": "codefuse-ai/C2LLM-0.5B",
        }, f, indent=2)

    size_mb = os.path.getsize(emb_path) / (1024 * 1024)
    print(f"\nDone! Model: {embeddings.shape[0]} tokens × {embeddings.shape[1]} dim = {size_mb:.0f} MB")
    print(f"Output directory: {output_dir}")


def main():
    args = parse_args()

    model, tokenizer = load_teacher(args.teacher, args.device, args.hf_mirror)
    teacher_hidden = model.config.hidden_size

    if args.dim > teacher_hidden:
        print(f"Warning: target dim {args.dim} > teacher hidden {teacher_hidden}, capping")
        args.dim = teacher_hidden

    # Step 1: Distill — collect hidden states for every token
    embeddings = distill_vocab(model, tokenizer, args.device, args.batch_size)

    # Step 2: PCA reduce
    if args.dim < teacher_hidden:
        embeddings = apply_pca(embeddings, args.dim)

    # Step 3: Save
    save_model(embeddings, tokenizer, args.output)


if __name__ == "__main__":
    main()

#!/bin/bash
set -e

RED='\033[0;31m'; GREEN='\033[0;32m'; CYAN='\033[0;36m'; NC='\033[0m'
BIN_DIR="${HOME}/.local/bin"
INSTALL_DIR="${BIN_DIR}"

echo -e "${CYAN}═══ rindex installer ═══${NC}"
echo ""

# ── Build ──
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")/rindex"

echo -e "${GREEN}[1/3] Building rindex...${NC}"
cd "$PROJECT_DIR"
cargo build --release
echo ""

# ── Install binary ──
echo -e "${GREEN}[2/3] Installing rindex to ${INSTALL_DIR}...${NC}"
mkdir -p "$INSTALL_DIR"
cp target/release/rindex "$INSTALL_DIR/rindex"
chmod +x "$INSTALL_DIR/rindex"
echo "  → rindex ${INSTALL_DIR}/rindex"
echo ""

# Ensure bin dir is on PATH
if ! echo "$PATH" | tr ':' '\n' | grep -qxF "$BIN_DIR"; then
    echo -e "${RED}Add ${BIN_DIR} to your PATH:${NC}"
    echo "  export PATH=\"\${HOME}/.local/bin:\$PATH\""
    echo ""
fi

# ── Distill & install model ──
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
MODEL_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/rindex/models/c2llm-static-256"
if [ ! -f "$MODEL_DIR/token_embeddings.safetensors" ]; then
    echo -e "${GREEN}[3/3] Building distilled model (~166MB)...${NC}"
    if [ -f "$PROJECT_ROOT/rindex-models/c2llm-static-256/token_embeddings.safetensors" ]; then
        mkdir -p "$(dirname "$MODEL_DIR")"
        cp -r "$PROJECT_ROOT/rindex-models/c2llm-static-256" "$(dirname "$MODEL_DIR")/"
        echo "  → model installed from rindex-models/"
    else
        mkdir -p "$MODEL_DIR"
        echo "  Model not found. Run: python scripts/distill.py --output ./rindex-models/c2llm-static-256/"
        echo "  Then: cp -r rindex-models/c2llm-static-256 $MODEL_DIR/"
    fi
else
    echo -e "${GREEN}[3/3] Model already cached (~166MB)${NC}"
fi

echo ""
echo -e "${CYAN}Done. Next: run 'rindex setup' in your project to configure MCP.${NC}"

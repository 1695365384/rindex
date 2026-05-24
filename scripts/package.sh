#!/bin/bash
# Package: build release + bundle model → one-click installer
set -e
cd "$(dirname "$0")/.."

VERSION=$(grep '^version' rindex/Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
OS=$(uname -s)
case "$OS" in
    Darwin)  PLATFORM="macos-arm64" ;;
    MINGW*|MSYS*) PLATFORM="windows-x64" ;;
    Linux)   PLATFORM="linux-x64" ;;
esac
RELEASE_DIR="pkg-${PLATFORM}"
MODEL_CACHE="$HOME/AppData/Roaming/rindex/models"
[ "$PLATFORM" = "macos-arm64" ] && MODEL_CACHE="$HOME/Library/Application Support/rindex/models"

echo "=== Building rindex v${VERSION} (${PLATFORM}) ==="
(cd rindex && cargo build --release)

[ -f rindex/target/release/rindex.exe ] && BIN="rindex/target/release/rindex.exe" BIN_NAME="rindex.exe" || BIN="rindex/target/release/rindex" BIN_NAME="rindex"

echo "=== Packaging ==="
rm -rf "$RELEASE_DIR"
mkdir -p "$RELEASE_DIR/model/c2llm-static-256"
cp "$BIN" "$RELEASE_DIR/$BIN_NAME"

# Copy distilled static model (from rindex-models/ or model cache)
if [ -d "rindex-models/c2llm-static-256" ]; then
    cp rindex-models/c2llm-static-256/* "$RELEASE_DIR/model/c2llm-static-256/"
    echo "Model: rindex-models/c2llm-static-256/"
elif [ -f "$MODEL_CACHE/c2llm-static-256/token_embeddings.safetensors" ]; then
    cp "$MODEL_CACHE/c2llm-static-256/"* "$RELEASE_DIR/model/c2llm-static-256/"
    echo "Model: $MODEL_CACHE/c2llm-static-256/"
else
    echo "WARN: distilled model not found — run 'python scripts/distill.py' first"
    echo "      then copy output to rindex-models/c2llm-static-256/"
fi

# ── Windows: install.bat ──
if [[ "$PLATFORM" == windows* ]]; then
    cp scripts/install.bat "$RELEASE_DIR/install.bat"
fi

# ═══════════════════════════════════════════
#  macOS: install.command (double-click)
# ═══════════════════════════════════════════
if [ "$PLATFORM" = "macos-arm64" ]; then
cat > "$RELEASE_DIR/install.command" << 'CMDOF'
#!/bin/bash
cd "$(dirname "$0")"

RED='\033[0;31m'; GREEN='\033[0;32m'; CYAN='\033[0;36m'; NC='\033[0m'
BIN="$HOME/.local/bin/rindex"
MODEL="$HOME/Library/Application Support/rindex/models"

echo ""
echo -e "${CYAN}═══════════════════════════════════${NC}"
echo -e "${CYAN}  rindex — Installer${NC}"
echo -e "${CYAN}═══════════════════════════════════${NC}"
echo ""

# Step 1: binary
echo -e "${CYAN}[1/3] Installing binary...${NC}"
mkdir -p "$HOME/.local/bin"
cp rindex "$BIN" && chmod +x "$BIN" && echo -e "       ${GREEN}[OK]${NC} $BIN" || { echo -e "       ${RED}[FAIL]${NC}"; exit 1; }

# Step 2: model
echo -e "${CYAN}[2/3] Installing model...${NC}"
if [ -f model/c2llm-static-256/token_embeddings.safetensors ]; then
    mkdir -p "$MODEL/c2llm-static-256"
    cp model/c2llm-static-256/* "$MODEL/c2llm-static-256/" && echo -e "       ${GREEN}[OK]${NC} $MODEL/c2llm-static-256/" || echo -e "       ${RED}[FAIL]${NC}"
else
    echo -e "       ${CYAN}[SKIP]${NC} model not bundled (~166MB, run distill.py to build)"
fi

# Step 3: verify
echo -e "${CYAN}[3/3] Verifying...${NC}"
"$BIN" --version &>/dev/null && echo -e "       ${GREEN}[OK]${NC} rindex is working" || echo -e "       ${YELLOW}[WARN]${NC} check PATH"

# PATH hint
if [[ ":$PATH:" != *":$HOME/.local/bin:"* ]]; then
    echo ""
    echo "  Add to ~/.zshrc:"
    echo "    export PATH=\"\$HOME/.local/bin:\$PATH\""
fi

echo ""
echo -e "${GREEN}═══════════════════════════════════${NC}"
echo -e "${GREEN}  Installation complete!${NC}"
echo -e "${GREEN}═══════════════════════════════════${NC}"
echo ""
echo "  Installed: rindex  →  $BIN"
echo "             model   →  $MODEL"
echo ""
echo "  Next: run 'rindex setup' in your project."
echo ""
read -p "Press Enter to close..."
CMDOF
chmod +x "$RELEASE_DIR/install.command"
fi

# ═══════════════════════════════════════════
#  CLI install.sh (both platforms)
# ═══════════════════════════════════════════
cat > "$RELEASE_DIR/install.sh" << 'SHEOF'
#!/bin/bash
set -e
case "$(uname -s)" in
    Darwin)
        BIN_DIR="$HOME/.local/bin"; MODEL_DIR="$HOME/Library/Application Support/rindex/models"; BIN="rindex" ;;
    MINGW*|MSYS*|MSYS_NT*)
        BIN_DIR="$HOME/.local/bin"; MODEL_DIR="${APPDATA:-$HOME/AppData/Roaming}/rindex/models"; BIN="rindex.exe" ;;
    Linux)
        BIN_DIR="$HOME/.local/bin"; MODEL_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/rindex/models"; BIN="rindex" ;;
esac

echo "rindex installer"
echo ""

mkdir -p "$BIN_DIR" "$MODEL_DIR"
cp "$BIN" "$BIN_DIR/" && chmod +x "$BIN_DIR/$BIN" 2>/dev/null || true
echo "  [OK] $BIN_DIR/$BIN"

if [ -f model/c2llm-static-256/token_embeddings.safetensors ]; then
    cp -r model/c2llm-static-256 "$MODEL_DIR/" && echo "  [OK] $MODEL_DIR/c2llm-static-256" || echo "  [FAIL] model copy"
else
    echo "  [--] model not bundled (~166MB, run distill.py to build)"
fi

"$BIN_DIR/$BIN" --version &>/dev/null && echo "  [OK] verified" || echo "  [!!] check PATH"

cd "$OLDPWD" 2>/dev/null || cd "$(pwd)"
if [ -f ".mcp.json" ] || [ -d ".claude" ]; then
    "$BIN_DIR/$BIN" setup 2>/dev/null && echo "  [OK] project configured" || true
else
    echo ""
    echo "  Run 'rindex setup' in your project."
fi
echo "  Done."
SHEOF
chmod +x "$RELEASE_DIR/install.sh"

# ═══════════════════════════════════════════
#  Package
# ═══════════════════════════════════════════
OUTPUT="rindex-v${VERSION}-${PLATFORM}"
rm -f "${OUTPUT}.zip" "${OUTPUT}.tar.gz"
case "$OS" in
    MINGW*|MSYS*) powershell.exe -Command "Compress-Archive -Path '$(cygpath -w "$RELEASE_DIR")' -DestinationPath '$(cygpath -w "${OUTPUT}.zip")'" && EXT="zip" ;;
    *)            tar -czf "${OUTPUT}.tar.gz" "$RELEASE_DIR" && EXT="tar.gz" ;;
esac
echo "=== ${OUTPUT}.${EXT} ==="
du -sh "${OUTPUT}.${EXT}"
rm -rf "$RELEASE_DIR"

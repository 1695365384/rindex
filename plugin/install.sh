#!/usr/bin/env bash
# rindex installer for Linux/macOS
# Usage: curl -fsSL https://raw.githubusercontent.com/<user>/llm-file-index/main/plugin/install.sh | bash

set -e

BIN_DIR="$HOME/.local/bin"
MODEL_DIR="$HOME/.local/share/rindex/models/c2llm-static-256"
mkdir -p "$BIN_DIR"
mkdir -p "$MODEL_DIR"

echo "=== rindex installer ==="

# Check if rindex is already installed
if command -v rindex &>/dev/null; then
    echo "[✓] rindex found in PATH: $(which rindex)"
else
    echo "[…] Installing rindex binary..."

    if command -v cargo &>/dev/null; then
        echo "    Using cargo install..."
        cargo install --git https://github.com/bundy-work/llm-file-index.git rindex
    else
        echo "    Downloading prebuilt binary..."

        OS=$(uname -s | tr '[:upper:]' '[:lower:]')
        ARCH=$(uname -m)
        case "$ARCH" in
            x86_64) ARCH="x86_64" ;;
            aarch64|arm64) ARCH="aarch64" ;;
            *) echo "Unsupported arch: $ARCH"; exit 1 ;;
        esac
        case "$OS" in
            linux) TARGET="${ARCH}-unknown-linux-gnu" ;;
            darwin) TARGET="${ARCH}-apple-darwin" ;;
            *) echo "Unsupported OS: $OS"; exit 1 ;;
        esac

        RELEASE_URL="https://github.com/bundy-work/llm-file-index/releases/latest/download/rindex-${TARGET}.tar.gz"
        curl -fsSL "$RELEASE_URL" -o /tmp/rindex.tar.gz
        tar -xzf /tmp/rindex.tar.gz -C "$BIN_DIR" rindex
        chmod +x "$BIN_DIR/rindex"
        rm /tmp/rindex.tar.gz
    fi

    if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
        echo "    Adding $BIN_DIR to PATH..."
        echo 'export PATH="$HOME/.local/bin:$PATH"' >> "$HOME/.bashrc"
        echo 'export PATH="$HOME/.local/bin:$PATH"' >> "$HOME/.zshrc" 2>/dev/null || true
        export PATH="$BIN_DIR:$PATH"
    fi

    echo "[✓] rindex installed to $BIN_DIR/rindex"
fi

# Install model if missing
if [ ! -f "$MODEL_DIR/model.safetensors" ]; then
    echo "[…] Downloading embedding model (~87 MB)..."

    if command -v cargo &>/dev/null; then
        echo "    Cargo detected. Run 'rindex backfill' after first use if model is missing."
    else
        OS=$(uname -s | tr '[:upper:]' '[:lower:]')
        ARCH=$(uname -m)
        case "$ARCH" in
            x86_64) ARCH="x86_64" ;;
            aarch64|arm64) ARCH="aarch64" ;;
            *) echo "Unsupported arch: $ARCH"; exit 1 ;;
        esac
        case "$OS" in
            linux) TARGET="${ARCH}-unknown-linux-gnu" ;;
            darwin) TARGET="${ARCH}-apple-darwin" ;;
            *) echo "Unsupported OS: $OS"; exit 1 ;;
        esac

        RELEASE_URL="https://github.com/bundy-work/llm-file-index/releases/latest/download/rindex-${TARGET}.tar.gz"
        curl -fsSL "$RELEASE_URL" -o /tmp/rindex-model.tar.gz
        tar -xzf /tmp/rindex-model.tar.gz -C /tmp rindex models/
        if [ -d "/tmp/models/c2llm-static-256" ]; then
            cp /tmp/models/c2llm-static-256/* "$MODEL_DIR/"
            echo "[✓] Model installed" -ForegroundColor Green
        else
            echo "[!] Model not found in release archive"
        fi
        rm -rf /tmp/rindex-model.tar.gz /tmp/rindex /tmp/models
    fi
else
    echo "[✓] Model already installed"
fi

# Register MCP server for Claude Code (user scope)
echo "[…] Registering rindex MCP server..."
claude mcp add --scope user rindex -- rindex 2>/dev/null || {
    echo "[!] Could not auto-register Claude Code MCP. Run manually:"
    echo "    claude mcp add --scope user rindex -- rindex"
}

echo ""
echo "=== Done! ==="
echo "Restart Claude Code to use rindex."
echo "For opencode: run 'rindex setup --opencode'"
echo "For Cursor:   run 'rindex setup --cursor'"

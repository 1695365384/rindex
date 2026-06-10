# rindex installer for Windows
# Usage: irm https://raw.githubusercontent.com/<user>/llm-file-index/main/plugin/install.ps1 | iex

$ErrorActionPreference = "Stop"

$BinDir = "$env:USERPROFILE\.local\bin"
$ModelDir = "$env:USERPROFILE\.local\share\rindex\models\c2llm-static-256"
New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
New-Item -ItemType Directory -Force -Path $ModelDir | Out-Null

Write-Host "=== rindex installer ===" -ForegroundColor Cyan

# Check if rindex is already installed
$rindexPath = (Get-Command rindex -ErrorAction SilentlyContinue).Source
if ($rindexPath) {
    Write-Host "[✓] rindex found: $rindexPath" -ForegroundColor Green
} else {
    Write-Host "[…] Installing rindex binary..." -ForegroundColor Yellow

    $cargoPath = (Get-Command cargo -ErrorAction SilentlyContinue).Source
    if ($cargoPath) {
        Write-Host "    Using cargo install..."
        cargo install --git https://github.com/bundy-work/llm-file-index.git rindex
    } else {
        Write-Host "    Downloading prebuilt binary..."
        $ReleaseUrl = "https://github.com/bundy-work/llm-file-index/releases/latest/download/rindex-x86_64-pc-windows.zip"
        $ZipFile = "$env:TEMP\rindex-install.zip"
        $ExtractDir = "$env:TEMP\rindex-install"
        Invoke-WebRequest -Uri $ReleaseUrl -OutFile $ZipFile
        Expand-Archive -Path $ZipFile -DestinationPath $ExtractDir -Force
        Copy-Item "$ExtractDir\rindex-x86_64-windows.exe" "$BinDir\rindex.exe" -Force
        Remove-Item $ZipFile -Force
        Remove-Item $ExtractDir -Recurse -Force
    }

    $currentPath = [Environment]::GetEnvironmentVariable("PATH", "User")
    if ($currentPath -notlike "*$BinDir*") {
        Write-Host "    Adding $BinDir to PATH..."
        [Environment]::SetEnvironmentVariable("PATH", "$currentPath;$BinDir", "User")
        $env:PATH = "$env:PATH;$BinDir"
    }

    Write-Host "[✓] rindex installed to $BinDir\rindex.exe" -ForegroundColor Green
}

# Install model if missing
if (-not (Test-Path "$ModelDir\model.safetensors")) {
    Write-Host "[…] Downloading embedding model (~87 MB)..." -ForegroundColor Yellow
    $cargoPath = (Get-Command cargo -ErrorAction SilentlyContinue).Source
    if ($cargoPath) {
        Write-Host "    Cargo detected. Model should be available after cargo build."
        Write-Host "    If model is missing, run: rindex backfill"
    } else {
        $ReleaseUrl = "https://github.com/bundy-work/llm-file-index/releases/latest/download/rindex-x86_64-pc-windows.zip"
        $ZipFile = "$env:TEMP\rindex-model-install.zip"
        $ExtractDir = "$env:TEMP\rindex-model-install"
        Invoke-WebRequest -Uri $ReleaseUrl -OutFile $ZipFile
        Expand-Archive -Path $ZipFile -DestinationPath $ExtractDir -Force
        if (Test-Path "$ExtractDir\models\c2llm-static-256") {
            Copy-Item "$ExtractDir\models\c2llm-static-256\*" $ModelDir -Force
            Write-Host "[✓] Model installed" -ForegroundColor Green
        } else {
            Write-Host "[!] Model not found in release archive" -ForegroundColor Yellow
        }
        Remove-Item $ZipFile -Force
        Remove-Item $ExtractDir -Recurse -Force
    }
} else {
    Write-Host "[✓] Model already installed" -ForegroundColor Green
}

# Register MCP server for Claude Code (user scope)
Write-Host "[…] Registering rindex MCP server..." -ForegroundColor Yellow
try {
    claude mcp add --scope user rindex -- rindex 2>$null
    Write-Host "[✓] Claude Code MCP registered" -ForegroundColor Green
} catch {
    Write-Host "[!] Could not auto-register Claude Code MCP. Run manually:" -ForegroundColor Yellow
    Write-Host "    claude mcp add --scope user rindex -- rindex"
}

Write-Host ""
Write-Host "=== Done! ===" -ForegroundColor Green
Write-Host "Restart Claude Code to use rindex."
Write-Host "For opencode: run 'rindex setup --opencode'"
Write-Host "For Cursor:   run 'rindex setup --cursor'"

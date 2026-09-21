# Amber Shield Installer — Windows (PowerShell)
# Usage: irm https://ambershield.app/install.ps1 | iex
# Requires: Windows 10/11 x64, PowerShell 5.1+

param(
    [string]$InstallDir = "$env:LOCALAPPDATA\AmberShield",
    [switch]$Force
)

$ErrorActionPreference = "Stop"
$REPO = "aurora-ember-bio-lab/Amber-Shield-releases"
$VERSION = "v0.1.0"
$BINARY = "amber-shield-lite.exe"
$ASSETS = @(
    "Amber Shield Lite_0.1.0_x64-setup.exe",
    "Amber Shield Lite_0.1.0_x64_en-US.msi"
)

function Write-Status($msg) { Write-Host "[amber-shield] $msg" -ForegroundColor Cyan }
function Write-Err($msg) { Write-Host "[amber-shield] ERROR: $msg" -ForegroundColor Red }

Write-Host ""
Write-Host "  ╔══════════════════════════════════════╗" -ForegroundColor DarkYellow
Write-Host "  ║     AMBER SHIELD INSTALLER           ║" -ForegroundColor DarkYellow
Write-Host "  ║     Local Security Intelligence      ║" -ForegroundColor DarkYellow
Write-Host "  ╚══════════════════════════════════════╝" -ForegroundColor DarkYellow
Write-Host ""

# Check architecture
if ([Environment]::Is64BitOperatingSystem -ne $true) {
    Write-Err "Amber Shield requires Windows x64. 32-bit is not supported."
    exit 1
}

# Create install directory
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
}

Write-Status "Installing Amber Shield $VERSION ..."

# Try winget first
$hasWinget = Get-Command winget -ErrorAction SilentlyContinue
if ($hasWinget -and -not $Force) {
    Write-Status "Installing via winget..."
    winget install --id AuroraEmberBioLab.AmberShield --version 0.1.0 --accept-package-agreements --accept-source-agreements 2>&1
    if ($LASTEXITCODE -eq 0) {
        Write-Status "Installed via winget successfully!"
        Write-Host ""
        Write-Host "  Run: amber-shield-lite" -ForegroundColor Green
        Write-Host ""
        exit 0
    }
    Write-Status "winget install failed, falling back to direct download..."
}

# Download the NSIS installer from GitHub Releases
$setupUrl = "https://github.com/$REPO/releases/download/$VERSION/Amber Shield Lite_0.1.0_x64-setup.exe"
$setupPath = Join-Path $InstallDir "AmberShield-Setup.exe"

Write-Status "Downloading $setupUrl ..."
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    Invoke-WebRequest -Uri $setupUrl -OutFile $setupPath -UseBasicParsing
} catch {
    Write-Err "Download failed: $_"
    Write-Status "Try downloading manually from: https://github.com/$REPO/releases"
    exit 1
}

Write-Status "Download complete. Running installer..."
Start-Process -FilePath $setupPath -ArgumentList "/S" -Wait

# Check if installed
$installedPath = Get-Command "amber-shield-lite" -ErrorAction SilentlyContinue
if ($installedPath) {
    Write-Status "Amber Shield installed successfully!"
} else {
    Write-Status "Installer launched. Follow the GUI steps to complete installation."
}

Write-Host ""
Write-Host "  Launch: amber-shield-lite" -ForegroundColor Green
Write-Host "  Source: https://github.com/$REPO" -ForegroundColor DarkGray
Write-Host "  License: MIT" -ForegroundColor DarkGray
Write-Host ""

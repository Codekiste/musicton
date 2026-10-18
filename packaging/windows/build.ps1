# Builds the Windows installer into dist\ using Inno Setup (preinstalled on GitHub's Windows runners).
#   .\packaging\windows\build.ps1 -Product musicton -Version 0.1.0
#   .\packaging\windows\build.ps1 -Product b303     -Version 0.1.0
param(
    [Parameter(Mandatory)][ValidateSet("musicton", "b303")][string]$Product,
    [Parameter(Mandatory)][string]$Version
)
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..\..")

if ($Product -eq "musicton") {
    cargo build --release -p musicton
} else {
    cargo xtask bundle b303-plugin --release
}
if ($LASTEXITCODE -ne 0) { throw "cargo failed" }

New-Item -ItemType Directory -Force dist | Out-Null
$iscc = (Get-Command iscc.exe -ErrorAction SilentlyContinue).Source
if (-not $iscc) { $iscc = "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" }
& $iscc "/DAppVersion=$Version" "packaging\windows\$Product.iss"
if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed" }
Get-ChildItem dist

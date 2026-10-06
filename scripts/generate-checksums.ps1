# Cryptographic Release Checksum Generator
# Generates SHA256SUMS.txt for release binaries, installers, and distribution archives.

param (
    [string]$TargetDir = "src-tauri/target/release"
)

Write-Host "==> Scanning for release binaries in: $TargetDir" -ForegroundColor Cyan

if (-not (Test-Path $TargetDir)) {
    Write-Warning "Directory '$TargetDir' does not exist. Please run a release build first."
    exit 0
}

$files = Get-ChildItem -Path $TargetDir -Recurse | Where-Object { 
    -not $_.PSIsContainer -and 
    ($_.Extension -in @(".exe", ".msi", ".zip", ".gz", ".gguf")) -and
    $_.FullName -notmatch '[\\/](build_script|deps|incremental|\.fingerprint)[\\/]' -and
    $_.Name -notmatch '^build-script'
}

if ($files.Count -eq 0) {
    Write-Warning "No binary artifacts found in $TargetDir."
    exit 0
}

$checksumFile = Join-Path $TargetDir "SHA256SUMS.txt"
$lines = @()

foreach ($file in $files) {
    $hash = (Get-FileHash -Path $file.FullName -Algorithm SHA256).Hash.ToLower()
    $relName = $file.Name
    $lines += "$hash  $relName"
    Write-Host "  $hash  $relName" -ForegroundColor Green
}

$lines | Out-File -FilePath $checksumFile -Encoding utf8
Write-Host "==> Successfully written SHA-256 checksums to: $checksumFile" -ForegroundColor Cyan

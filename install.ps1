# Installs Ivy from the latest GitHub release: downloads the setup file, checks it, runs it, and starts Ivy.
# Paste into PowerShell:
#
#   irm https://raw.githubusercontent.com/raj-7676/IVY-/main/install.ps1 | iex
#
# Same result as downloading the setup file and double-clicking it. Ivy then downloads its speech model
# (about 2.5 GB, one time) itself, with progress in its window.
# Nothing is sent anywhere; it only downloads Ivy's own release files from GitHub.

$ErrorActionPreference = 'Stop'

function Install-Ivy {
    $repo = 'raj-7676/IVY-'
    if (-not [Environment]::Is64BitOperatingSystem) { throw 'Ivy needs 64-bit Windows.' }
    if (-not (Get-Command curl.exe -ErrorAction SilentlyContinue)) { throw 'curl.exe is missing (it ships with Windows 10 1803 and later).' }

    Write-Host 'Ivy installer' -ForegroundColor DarkYellow
    Write-Host 'Finding the latest release...'
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    $release = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest" -Headers @{ 'User-Agent' = 'ivy-install' }
    $tag = $release.tag_name
    $setup = $release.assets | Where-Object { $_.name -like '*_x64-setup.exe' } | Select-Object -First 1
    if (-not $setup) { throw "Release $tag has no setup file." }

    # Room for Ivy and the speech model it downloads next (both live in Local AppData).
    $freeGB = [math]::Floor((Get-PSDrive ($env:LOCALAPPDATA.Substring(0, 1))).Free / 1GB)
    if ($freeGB -lt 3) { throw "Not enough disk space: Ivy and its speech model need about 3 GB free, drive has $freeGB GB." }

    # The release's own checksum file; the setup file must match it AND GitHub's record of the upload.
    $expected = @()
    $sumsAsset = $release.assets | Where-Object { $_.name -eq 'SHA256SUMS.txt' }
    if ($sumsAsset) {
        $sums = (& curl.exe -sL --fail --retry 3 $sumsAsset.browser_download_url) -join "`n"
        if ($LASTEXITCODE -ne 0) { throw 'Could not download SHA256SUMS.txt. Check your internet and run the command again.' }
        if ($sums -match "(?m)^([0-9a-fA-F]{64})\s+\*?$([regex]::Escape($setup.name))\s*$") { $expected += $Matches[1].ToLower() }
    }
    if ($setup.digest) { $expected += ($setup.digest -replace '^sha256:', '').ToLower() }
    if (-not $expected) { throw "No checksum published for $($setup.name); not installing an unchecked file." }

    $file = Join-Path $env:TEMP $setup.name
    Write-Host ("Downloading {0} ({1:N0} MB)" -f $setup.name, ($setup.size / 1MB))
    # A dropped connection (curl exit 18) isn't something curl's own --retry covers: run it again, resuming.
    # Already complete from an earlier run: curl -C - would ask for bytes past the end and fail.
    $complete = { (Test-Path $file) -and (Get-Item $file).Length -eq $setup.size }
    for ($try = 1; -not (& $complete) -and $try -le 5; $try++) {
        if ($try -gt 1) { Start-Sleep 3 }
        & curl.exe -L --fail --retry 5 --retry-delay 3 -C - -o $file $setup.browser_download_url
    }
    if (-not (& $complete)) { throw "Download of $($setup.name) stopped (curl exit $LASTEXITCODE). Run the command again to resume." }
    $hash = (Get-FileHash $file -Algorithm SHA256).Hash.ToLower()
    foreach ($e in $expected) {
        if ($hash -ne $e) {
            Remove-Item $file -Force
            throw "$($setup.name) doesn't match its published checksum, so it was deleted. Run the command again."
        }
    }

    Write-Host 'Installing Ivy...'
    # Fully gone before setup runs: setup started right after Stop-Process left the old app.exe in place.
    Get-Process app -ErrorAction SilentlyContinue | Where-Object { $_.Path -like '*\Ivy\app.exe' } | ForEach-Object {
        $_.Kill()
        $_.WaitForExit(10000) | Out-Null
    }
    Start-Sleep 1
    $p = Start-Process $file -ArgumentList '/S' -Wait -PassThru
    if ($p.ExitCode -ne 0) { throw "Setup failed (exit code $($p.ExitCode)). The setup file is kept at $file." }
    Remove-Item $file -Force

    $exe = Join-Path $env:LOCALAPPDATA 'Ivy\app.exe'
    if (Test-Path $exe) { Start-Process $exe }
    Write-Host "Ivy $tag is installed." -ForegroundColor Green
    if (-not (Test-Path (Join-Path $env:LOCALAPPDATA 'Ivy\models\ivy-lite\ivy-lite-Q8_0.gguf'))) {
        Write-Host "Ivy's window now downloads its speech model (one time only, about 2.5 GB), then opens its setup." -ForegroundColor Green
    } else {
        Write-Host 'Hold Alt + Space anywhere and talk.' -ForegroundColor Green
    }
}

try { Install-Ivy } catch { Write-Host $_.Exception.Message -ForegroundColor Red }

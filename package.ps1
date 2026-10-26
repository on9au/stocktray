# Builds the release exe and its installer, dist\stocktray-<version>-setup.exe. Used locally and by CI.
# Needs Inno Setup: winget install JRSoftware.InnoSetup
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

$version = (cargo metadata --no-deps --format-version 1 | ConvertFrom-Json).packages[0].version
cargo build --release
if ($LASTEXITCODE) { exit $LASTEXITCODE }

$iscc = (Get-Command iscc -ErrorAction Ignore)?.Source ?? (Get-ChildItem -ErrorAction Ignore `
        "$env:LOCALAPPDATA\Programs\Inno Setup*\ISCC.exe", "${env:ProgramFiles(x86)}\Inno Setup*\ISCC.exe" |
        Select-Object -Last 1).FullName
if (-not $iscc) { throw 'Inno Setup not found: winget install JRSoftware.InnoSetup' }
& $iscc /Q "/DAppVersion=$version" installer\stocktray.iss
exit $LASTEXITCODE

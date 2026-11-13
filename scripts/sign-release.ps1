<#
.SYNOPSIS
  Signs the packages of a draft release with the update key and publishes it.

.DESCRIPTION
  1. Downloads the Windows package, the Windows installer and the macOS
     package of the draft release v<Version> with their .sha256 files.
  2. Checks the checksums.
  3. Signs each file with the secret key on THIS PC (minisign asks for the
     password). The signed ("trusted") comment names system and version,
     e.g. "versolopdf-windows 1.5.0", so a signature can't be used for
     another file or version.
  4. Checks the signatures with keys\update-key.pub (the key built into the
     program).
  5. Uploads the .minisig files and - after asking - publishes the release.

  Needs: gh (logged in), minisign.   Usage:  .\scripts\sign-release.ps1 1.5.0
#>
param([Parameter(Mandatory = $true)][string]$Version)
# Native tools write progress to stderr; failures are caught via $LASTEXITCODE.
$ErrorActionPreference = 'Continue'
$repo = 'MrMarkus42/VersoloPDF'
$root = Split-Path $PSScriptRoot -Parent
$pub = Join-Path $root 'keys\update-key.pub'
$sec = Join-Path $env:USERPROFILE '.minisign\versolopdf.key'
$tag = "v$Version"
if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw 'Version must look like 1.2.3' }
$minisign = (Get-Command minisign -ErrorAction SilentlyContinue).Source
if (-not $minisign) { throw 'minisign not found (winget install jedisct1.minisign)' }
if (-not (Get-Command gh -ErrorAction SilentlyContinue)) { throw 'gh not found' }
if (-not (Test-Path $sec)) { throw "Secret key not found: $sec (run scripts\new-update-key.ps1 first)" }
if ((Get-Content $pub -Raw) -match 'NOT-CONFIGURED') { throw 'keys\update-key.pub is still the placeholder' }

# File and the signed comment the program expects (src/update.rs).
$files = [ordered]@{
    "VersoloPDF-$Version-windows-x64.zip"       = "versolopdf-windows $Version"
    "VersoloPDF-$Version-macos.zip"             = "versolopdf-macos $Version"
    "VersoloPDF-$Version-windows-x64-setup.exe" = "versolopdf-windows-setup $Version"
}
$work = Join-Path $env:TEMP "versolopdf-sign-$Version"
Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
New-Item -ItemType Directory $work | Out-Null

Write-Host "Tag $tag points to:" -ForegroundColor Cyan
gh api "repos/$repo/commits/$tag" --jq .sha
foreach ($f in $files.Keys) {
    gh release download $tag --repo $repo --dir $work --pattern $f --pattern "$f.sha256"
    if ($LASTEXITCODE -ne 0) { throw "download of $f failed - is it in the draft release?" }
}

Write-Host 'Checking checksums ...' -ForegroundColor Cyan
foreach ($f in $files.Keys) {
    $want = ((Get-Content (Join-Path $work "$f.sha256") -Raw).Trim() -split '\s+')[0].ToLower()
    $got = (Get-FileHash (Join-Path $work $f) -Algorithm SHA256).Hash.ToLower()
    if ($want -ne $got) { throw "checksum mismatch: $f" }
    Write-Host "  ok  $f"
}

Write-Host 'Signing (minisign asks for the key password for each file) ...' -ForegroundColor Cyan
foreach ($f in $files.Keys) {
    & $minisign -S -s $sec -m (Join-Path $work $f) -t $files[$f]
    if ($LASTEXITCODE -ne 0) { throw "signing $f failed" }
    & $minisign -V -p $pub -m (Join-Path $work $f)
    if ($LASTEXITCODE -ne 0) { throw "signature check failed for $f - is keys\update-key.pub the matching public key?" }
}

$sigs = $files.Keys | ForEach-Object { Join-Path $work "$_.minisig" }
gh release upload $tag --repo $repo --clobber @sigs
if ($LASTEXITCODE -ne 0) { throw 'upload failed' }
Write-Host "Signed and uploaded. Draft: https://github.com/$repo/releases/tag/$tag" -ForegroundColor Green
$a = Read-Host 'Publish the release now? (yes/no)'
if ($a -eq 'yes') {
    gh release edit $tag --repo $repo --draft=false --latest
    if ($LASTEXITCODE -ne 0) { throw 'publishing failed' }
    Write-Host 'Published. Installed copies find it at their next check.' -ForegroundColor Green
}
Remove-Item -Recurse -Force $work

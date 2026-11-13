<#
.SYNOPSIS
  Creates the update signing key of VersoloPDF (run ONCE, on the developer's own PC).

.DESCRIPTION
  * The secret key goes to %USERPROFILE%\.minisign\versolopdf.key, protected by
    a password that minisign asks for twice. It never goes into the
    repository, onto GitHub or into a cloud folder (see .gitignore).
  * The public key is written to keys\update-key.pub. It is compiled into the
    program, which then only installs updates signed with the secret key.
  * Back up the secret key AND its password (e.g. in a password manager).
    Without them installed copies can never get an update again.

  Needs minisign:  winget install jedisct1.minisign
#>
# minisign writes its questions to stderr; failures are caught via $LASTEXITCODE.
$ErrorActionPreference = 'Continue'
$root = Split-Path $PSScriptRoot -Parent
$pub = Join-Path $root 'keys\update-key.pub'
$secDir = Join-Path $env:USERPROFILE '.minisign'
$sec = Join-Path $secDir 'versolopdf.key'

$minisign = (Get-Command minisign -ErrorAction SilentlyContinue).Source
if (-not $minisign) { throw 'minisign not found. Install it: winget install jedisct1.minisign' }
if (Test-Path $sec) { throw "A secret key already exists: $sec  (delete it only if you really want a new key - installed copies would no longer accept updates)" }
if ((Test-Path $pub) -and ((Get-Content $pub -Raw) -notmatch 'NOT-CONFIGURED')) {
    $a = Read-Host 'keys\update-key.pub already holds a real key. Replace it? Installed copies will then reject updates! (yes/no)'
    if ($a -ne 'yes') { exit 1 }
}
New-Item -ItemType Directory -Force $secDir | Out-Null
New-Item -ItemType Directory -Force (Split-Path $pub) | Out-Null
& $minisign -G -f -p $pub -s $sec -c 'VersoloPDF update key'
if ($LASTEXITCODE -ne 0) { throw 'minisign failed' }
Write-Host ''
Write-Host "Secret key : $sec   <- back it up together with its password, never share it" -ForegroundColor Yellow
Write-Host "Public key : $pub   <- part of the program (commit it)" -ForegroundColor Green
Get-Content $pub

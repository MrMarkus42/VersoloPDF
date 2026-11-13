<#
.SYNOPSIS
  Builds the Windows installer (VersoloPDF-X.Y.Z-windows-x64-setup.exe) from
  the release package VersoloPDF-X.Y.Z-windows-x64.zip.

.DESCRIPTION
  Runs on Windows with Inno Setup 6 (winget install JRSoftware.InnoSetup).
  The package is made by scripts/package-windows.sh; the installer contains
  exactly its files. The setup lands next to the zip, with a .sha256 file.

  Usage:  .\scripts\make-installer.ps1 C:\path\VersoloPDF-1.5.0-windows-x64.zip
#>
param([Parameter(Mandatory = $true)][string]$Zip)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$iss = Join-Path $root 'packaging\windows\VersoloPDF.iss'
$Zip = (Resolve-Path $Zip).Path
$name = Split-Path $Zip -Leaf
if ($name -notmatch '^VersoloPDF-(\d+\.\d+\.\d+)-windows-x64\.zip$') { throw "unexpected package name: $name" }
$version = $Matches[1]

$iscc = @(
    (Get-Command iscc -ErrorAction SilentlyContinue).Source,
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $iscc) { throw 'Inno Setup 6 not found (winget install JRSoftware.InnoSetup)' }

$work = Join-Path $env:TEMP "versolopdf-setup-$version"
Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
Expand-Archive -Path $Zip -DestinationPath $work
$source = Get-ChildItem $work -Recurse -Filter 'VersoloPDF.exe' | Select-Object -First 1
if (-not $source) { throw 'VersoloPDF.exe not found in the package' }
$out = Split-Path $Zip -Parent

& $iscc /Q "/DAppVersion=$version" "/DSourceDir=$($source.DirectoryName)" "/DOutputDir=$out" $iss
if ($LASTEXITCODE -ne 0) { throw 'ISCC failed' }
Remove-Item -Recurse -Force $work

$setup = Join-Path $out "VersoloPDF-$version-windows-x64-setup.exe"
$hash = (Get-FileHash $setup -Algorithm SHA256).Hash.ToLower()
# Same form as sha256sum: "<hash>  <file name>".
[IO.File]::WriteAllText("$setup.sha256", "$hash  $(Split-Path $setup -Leaf)`n", [Text.UTF8Encoding]::new($false))
Write-Host "Installer: $setup" -ForegroundColor Green

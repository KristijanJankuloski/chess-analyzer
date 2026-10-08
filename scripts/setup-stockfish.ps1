# Downloads Stockfish into engines\stockfish.exe (gitignored).
# Usage: scripts\setup-stockfish.ps1 [-Version sf_19] [-Destination <dir>]
param(
    [string]$Version = "sf_19",
    [string]$Destination = (Join-Path $PSScriptRoot "..\engines")
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"   # the progress bar makes Invoke-WebRequest very slow

$arch = if ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq "Arm64") { "arm64" } else { "x86-64" }
$asset = "stockfish-windows-$arch-universal.zip"
$url = "https://github.com/official-stockfish/Stockfish/releases/download/$Version/$asset"

$work = Join-Path ([System.IO.Path]::GetTempPath()) ("stockfish-" + [System.Guid]::NewGuid())
New-Item -ItemType Directory -Path $work | Out-Null
try {
    $zip = Join-Path $work $asset
    Write-Host "Downloading $url"
    Invoke-WebRequest -Uri $url -OutFile $zip

    $extracted = Join-Path $work "extracted"
    Expand-Archive -Path $zip -DestinationPath $extracted

    $exe = Get-ChildItem -Path $extracted -Recurse -File -Filter "stockfish*.exe" | Select-Object -First 1
    if (-not $exe) { throw "No Stockfish executable found inside $asset" }

    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
    $target = Join-Path (Resolve-Path $Destination) "stockfish.exe"
    Copy-Item -Path $exe.FullName -Destination $target -Force

    # A UTF-8 console would prepend a byte-order mark to what we send ("﻿uci" is not a UCI command),
    # so switch the console input encoding to UTF-8 without a BOM before starting the process.
    [Console]::InputEncoding = New-Object System.Text.UTF8Encoding($false)
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $target
    $info.UseShellExecute = $false
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $process = [System.Diagnostics.Process]::Start($info)
    $process.StandardInput.WriteLine("uci")
    $process.StandardInput.WriteLine("quit")
    $reply = $process.StandardOutput.ReadToEnd()
    $process.WaitForExit()
    if ($reply -notmatch "uciok") { throw "$target did not answer the UCI handshake" }
    $name = ($reply -split "`r?`n" | Where-Object { $_ -like "id name*" } | Select-Object -First 1)
    Write-Host "Installed $name at $target"
}
finally {
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}

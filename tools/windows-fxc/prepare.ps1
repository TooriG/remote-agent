[CmdletBinding()]
param(
    [string]$CacheRoot
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
Set-StrictMode -Version Latest
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
if (-not $CacheRoot) { $CacheRoot = $env:AGENT_FXC_CACHE_ROOT }
if (-not $CacheRoot) { $CacheRoot = Join-Path $PSScriptRoot "../../target/tool-cache/windows-fxc" }

function Get-PinnedFileHash([string]$Path) {
    # Cargo's PowerShell child may not discover the Get-FileHash script module.
    $sha = [Security.Cryptography.SHA256]::Create()
    $stream = [IO.File]::OpenRead($Path)
    try { [BitConverter]::ToString($sha.ComputeHash($stream)).Replace("-", "").ToLowerInvariant() }
    finally { $stream.Dispose(); $sha.Dispose() }
}

$pin = Get-Content -LiteralPath (Join-Path $PSScriptRoot "manifest.json") -Raw | ConvertFrom-Json
$directory = [IO.Path]::GetFullPath((Join-Path $CacheRoot $pin.version))
if (-not (Test-Path -LiteralPath $directory)) {
    New-Item -ItemType Directory -Path $CacheRoot -Force | Out-Null
    $pending = Join-Path $CacheRoot ([guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $pending | Out-Null
    try {
        $package = $pin.package.ToLowerInvariant()
        $archive = Join-Path $pending "sdk.nupkg"
        Invoke-WebRequest -UseBasicParsing -Uri "https://api.nuget.org/v3-flatcontainer/$package/$($pin.version)/$package.$($pin.version).nupkg" -OutFile $archive
        if ((Get-PinnedFileHash $archive) -ne $pin.sha256) {
            throw "Pinned Windows SDK package checksum mismatch."
        }
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $zip = [IO.Compression.ZipFile]::OpenRead($archive)
        try {
            foreach ($file in $pin.files) {
                $entry = $zip.GetEntry($file.path)
                if ($null -eq $entry) { throw "Pinned SDK entry is missing: $($file.path)" }
                [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, (Join-Path $pending $entry.Name))
            }
        } finally { $zip.Dispose() }
        Remove-Item -LiteralPath $archive
        try {
            [IO.Directory]::Move($pending, $directory)
        } catch [IO.IOException] {
            # A concurrent build may have published the same immutable package.
            if (-not (Test-Path -LiteralPath $directory)) { throw }
        }
    } finally {
        if (Test-Path -LiteralPath $pending) { Remove-Item -LiteralPath $pending -Recurse -Force }
    }
}
foreach ($file in $pin.files) {
    $path = Join-Path $directory ([IO.Path]::GetFileName($file.path))
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Pinned shader compiler file is missing: $path"
    }
    if ((Get-PinnedFileHash $path) -ne $file.sha256) {
        throw "Pinned shader compiler checksum mismatch: $path"
    }
}
Join-Path $directory "fxc.exe"

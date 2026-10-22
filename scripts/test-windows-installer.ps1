[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Installer,
    [switch]$HideShaderSources
)
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
$installerPath = (Resolve-Path -LiteralPath $Installer).Path
$testRoot = Join-Path ([IO.Path]::GetTempPath()) ("bex-install-test-" + [guid]::NewGuid())
$installDirectory = Join-Path $testRoot "app"
$previousEnvironment = @{}
$hidden = @()
New-Item -ItemType Directory -Path $testRoot | Out-Null
try {
    $install = Start-Process -FilePath $installerPath -ArgumentList @("/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CURRENTUSER", ('/DIR="' + $installDirectory + '"')) -WindowStyle Hidden -Wait -PassThru
    if ($install.ExitCode -ne 0) { throw "Installer failed: $($install.ExitCode)" }
    foreach ($name in @("bex-desktop.exe", "host-daemon.exe", "bex-provider-supervisor.exe", "agent-cli.exe", "bex-claude-sdk.mjs", "build-info.json")) {
        if (-not (Test-Path -LiteralPath (Join-Path $installDirectory $name))) { throw "Installer omitted $name" }
    }
    $metadata = Get-Content -LiteralPath (Join-Path $installDirectory "build-info.json") -Raw | ConvertFrom-Json
    $binaryInfo = & (Join-Path $installDirectory "bex-desktop.exe") --build-info | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or $binaryInfo.profile -ne "release" -or $binaryInfo.debugAssertions -or $binaryInfo.revision -ne $metadata.revision) {
        throw "Installed executable is not the expected release build."
    }
    if ($HideShaderSources) {
        $cargoRoot = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE ".cargo" }
        $registryRoot = [IO.Path]::GetFullPath((Join-Path $cargoRoot "registry/src")) + [IO.Path]::DirectorySeparatorChar
        foreach ($crate in Get-ChildItem -LiteralPath $registryRoot -Directory | Get-ChildItem -Directory -Filter "gpui-pre-windows-*") {
            $source = [IO.Path]::GetFullPath((Join-Path $crate.FullName "src"))
            if (-not $source.StartsWith($registryRoot, [StringComparison]::OrdinalIgnoreCase)) { throw "Shader source outside Cargo registry." }
            $destination = $source + ".install-test-hidden"
            Move-Item -LiteralPath $source -Destination $destination
            $hidden += @{ source = $source; destination = $destination }
        }
        if ($hidden.Count -eq 0) { throw "No GPUI shader source found to hide; regression test would be ineffective." }
    }
    foreach ($entry in @{
        BEX_STATE_DIR = (Join-Path $testRoot "state")
        BEX_ISOLATED_HOST = "1"
        CODEX_HOME = (Join-Path $testRoot "codex")
    }.GetEnumerator()) {
        $previousEnvironment[$entry.Key] = [Environment]::GetEnvironmentVariable($entry.Key, "Process")
        [Environment]::SetEnvironmentVariable($entry.Key, $entry.Value, "Process")
    }
    New-Item -ItemType Directory -Path $env:CODEX_HOME | Out-Null
    $desktop = Start-Process -FilePath (Join-Path $installDirectory "bex-desktop.exe") -WorkingDirectory $testRoot -WindowStyle Hidden -PassThru
    $deadline = (Get-Date).AddSeconds(45)
    $windowOpened = $false
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 500
        $desktop.Refresh()
        if ($desktop.HasExited) { throw "Installed desktop exited during startup: $($desktop.ExitCode)" }
        if ($desktop.MainWindowHandle -ne [IntPtr]::Zero) { $windowOpened = $true; break }
    }
    if (-not $windowOpened) { throw "Installed desktop never opened a native window." }
    Start-Sleep -Seconds 8
    $desktop.Refresh()
    if ($desktop.HasExited) { throw "Installed desktop exited after opening its window." }
    $logs = @(Get-ChildItem -LiteralPath (Join-Path $testRoot "state") -Filter "*.jsonl" -Recurse -ErrorAction SilentlyContinue)
    foreach ($log in $logs) {
        foreach ($line in Get-Content -LiteralPath $log.FullName) {
            $record = $line | ConvertFrom-Json
            if ($record.operation -eq "panic") { throw "Installed desktop/Host panicked: $($record.message)" }
        }
    }
    Write-Output "PASS: installed release $($metadata.revision) opened a native window without build-time shader sources."
} finally {
    # Only stop processes executing from this test's unique installation directory.
    foreach ($process in Get-Process -ErrorAction SilentlyContinue) {
        if ($process.Path -and $process.Path.StartsWith($installDirectory + '\', [StringComparison]::OrdinalIgnoreCase)) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        }
    }
    foreach ($entry in $hidden) { Move-Item -LiteralPath $entry.destination -Destination $entry.source }
    foreach ($entry in $previousEnvironment.GetEnumerator()) { [Environment]::SetEnvironmentVariable($entry.Key, $entry.Value, "Process") }
    Write-Output "Installer test evidence retained at $testRoot"
}

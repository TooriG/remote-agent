$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$prepare = Join-Path $PSScriptRoot "prepare.ps1"
$installed = if ($env:OS -eq "Windows_NT") {
    # Exercise the build script's fresh Windows PowerShell process, including -File defaults.
    & powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $prepare
    if ($LASTEXITCODE -ne 0) { throw "Pinned FXC preparation failed in a fresh process." }
} else { & $prepare }
$temporary = Join-Path ([IO.Path]::GetTempPath()) ("bex fxc " + [guid]::NewGuid())
New-Item -ItemType Directory -Path $temporary | Out-Null
try {
    $badTools = Join-Path $temporary "bad package"
    New-Item -ItemType Directory -Path $badTools | Out-Null
    Copy-Item -LiteralPath $prepare -Destination $badTools
    $pin = Get-Content -LiteralPath (Join-Path $PSScriptRoot "manifest.json") -Raw | ConvertFrom-Json
    $pin.sha256 = "0" * 64
    $pin | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $badTools "manifest.json") -Encoding UTF8
    $badCache = Join-Path $temporary "untrusted cache"
    $rejected = $false
    try { & (Join-Path $badTools "prepare.ps1") -CacheRoot $badCache | Out-Null }
    catch { $rejected = $_.Exception.Message -match "Pinned Windows SDK package checksum mismatch" }
    if (-not $rejected -or (Test-Path -LiteralPath (Join-Path $badCache $pin.version))) {
        throw "An SDK package with an incorrect hash was not rejected before publication."
    }

    $cache = Join-Path $temporary "cache"
    New-Item -ItemType Directory -Path $cache | Out-Null
    Copy-Item -LiteralPath ([IO.Path]::GetDirectoryName($installed)) -Destination $cache -Recurse
    $compiler = & $prepare -CacheRoot $cache
    if ($compiler -eq $installed) { throw "Isolated cache was not used." }

    $dll = Join-Path ([IO.Path]::GetDirectoryName($compiler)) "d3dcompiler_47.dll"
    $original = [IO.File]::ReadAllBytes($dll)
    foreach ($failure in @("missing", "corrupt")) {
        if ($failure -eq "missing") { Remove-Item -LiteralPath $dll }
        else { [IO.File]::WriteAllBytes($dll, [byte[]]@(0, 1, 2, 3)) }
        $rejected = $false
        try { & $prepare -CacheRoot $cache | Out-Null }
        catch { $rejected = $_.Exception.Message -match "Pinned shader compiler (file is missing|checksum mismatch)" }
        finally { [IO.File]::WriteAllBytes($dll, $original) }
        if (-not $rejected) { throw "$failure compiler DLL was not rejected." }
    }

    if ($env:OS -eq "Windows_NT") {
        $hashes = @()
        foreach ($name in @("first location", "second location")) {
            $directory = Join-Path $temporary $name
            New-Item -ItemType Directory -Path $directory | Out-Null
            $source = Join-Path $directory "shader.hlsl"
            $output = Join-Path $directory "shader.bin"
            [IO.File]::WriteAllText($source, "float4 main(float4 position : POSITION) : SV_Position { return position; }")
            & $compiler /nologo /T vs_4_1 /E main /O3 /Fo $output $source
            if ($LASTEXITCODE -ne 0) { throw "Pinned FXC could not compile the test shader." }
            $bytes = [IO.File]::ReadAllBytes($output)
            if ([Text.Encoding]::ASCII.GetString($bytes, 0, 4) -ne "DXBC") { throw "Invalid shader bytecode." }
            $hashes += (Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash
        }
        if ($hashes[0] -ne $hashes[1]) { throw "Shader output depends on its source directory." }
        Write-Output "Pinned FXC integrity and reproducible shader compilation passed."
    } else {
        Write-Output "Pinned FXC integrity passed; shader compilation requires Windows."
    }
} finally { Remove-Item -LiteralPath $temporary -Recurse -Force }

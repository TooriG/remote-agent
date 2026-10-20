$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot "update-policy.ps1")

function Assert-Rejected([scriptblock]$Action, [string]$Message) {
    try { & $Action | Out-Null }
    catch { if ($_.Exception.Message -match $Message) { return }; throw }
    throw "Expected rejection: $Message"
}

$versions = @("10.0.9.0", "10.0.10.0", "10.0.11.0-preview", "11.0.0.0-rc", "invalid")
foreach ($order in @($versions, @($versions[4..0]))) {
    if ((Select-StableSdkVersion $order "10.0.8.0") -ne "10.0.10.0") {
        throw "Must select the numerically newest stable SDK regardless of feed order."
    }
}
foreach ($current in @("10.0.10.0", "10.0.12.0")) {
    if ((Select-StableSdkVersion $versions $current) -ne $current) { throw "Must not downgrade or update an equal pin." }
}
if ((Select-StableSdkVersion @("10.0.10.0", "11.0.0", "11.0.1-preview") "10.0.10.0") -ne "11.0.0") {
    throw "NuGet-normalized three-component stable versions must be eligible."
}
Assert-Rejected { Select-StableSdkVersion @() "10.0.1.0" } "No stable"
Assert-Rejected { Select-StableSdkVersion @("11.0.0.0-preview") "10.0.1.0" } "No stable"

$compiler = "c/bin/10.0.28000.0/x64/fxc.exe"
$dll = "c/bin/10.0.28000.0/x64/d3dcompiler_47.dll"
$entries = @($dll, "c/bin/10.0.28000.0/arm64/fxc.exe", "c/bin/10.0.28000.0/x86/d3dcompiler_47.dll", $compiler)
$selected = @(Select-FxcPaths $entries)
if ($selected.Count -ne 2 -or $selected[0] -cne $compiler -or $selected[1] -cne $dll) {
    throw "Must select only the x64 compiler and matching DLL, in manifest order."
}
Assert-Rejected { Select-FxcPaths @($compiler) } "exactly one.*d3dcompiler"
Assert-Rejected { Select-FxcPaths @($dll) } "exactly one.*fxc"
Assert-Rejected { Select-FxcPaths @($compiler, $dll, "c/bin/10.0.30000.0/x64/fxc.exe") } "exactly one.*fxc"
Assert-Rejected { Select-FxcPaths @($compiler, $dll, $dll) } "exactly one.*d3dcompiler"
Assert-Rejected { Select-FxcPaths @($compiler, "c/bin/10.0.30000.0/x64/d3dcompiler_47.dll") } "same SDK directory"

# Run the real updater against a small archive and a simulated NuGet feed.
$temporary = Join-Path ([IO.Path]::GetTempPath()) ("bex update test " + [guid]::NewGuid())
New-Item -ItemType Directory -Path $temporary | Out-Null
try {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot "update.ps1"), (Join-Path $PSScriptRoot "update-policy.ps1") -Destination $temporary
    $manifest = Join-Path $temporary "manifest.json"
    [IO.File]::WriteAllText($manifest, '{"package":"Microsoft.Windows.SDK.CPP","version":"10.0.1.0","sha256":"old","files":[]}')
    $sdkFixture = Join-Path $temporary "fixture.nupkg"
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::Open($sdkFixture, [IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($fixture in @(@($compiler, "abc"), @($dll, "hello"))) {
            $writer = [IO.StreamWriter]::new($zip.CreateEntry($fixture[0]).Open(), [Text.UTF8Encoding]::new($false))
            try { $writer.Write($fixture[1]) } finally { $writer.Dispose() }
        }
    } finally { $zip.Dispose() }

    $downloadFailed = $false
    $downloads = [Collections.Generic.List[string]]::new()
    function Invoke-RestMethod([string]$Uri) {
        if ($Uri -like "*/index.json") {
            return @{ versions = @("10.0.1.0", "10.0.2.0", "10.0.3.0", "10.0.4.0-preview") }
        }
        if ($Uri -like "*/10.0.3.0.json") { return @{ listed = $false } }
        if ($Uri -like "*/10.0.2.0.json") { return @{ listed = $true } }
        throw "Unexpected metadata request: $Uri"
    }
    function Invoke-WebRequest([switch]$UseBasicParsing, [string]$Uri, [string]$OutFile) {
        if ($downloadFailed) { throw "Simulated download failure" }
        $downloads.Add($Uri)
        Copy-Item -LiteralPath $sdkFixture -Destination $OutFile
    }

    $original = [IO.File]::ReadAllText($manifest)
    $downloadFailed = $true
    Assert-Rejected { & (Join-Path $temporary "update.ps1") } "Simulated download failure"
    if ([IO.File]::ReadAllText($manifest) -cne $original) { throw "A failed download changed the pin." }
    $downloadFailed = $false
    & (Join-Path $temporary "update.ps1")
    $updated = Get-Content -LiteralPath $manifest -Raw | ConvertFrom-Json
    if ($updated.package -cne "Microsoft.Windows.SDK.CPP" -or $updated.version -ne "10.0.2.0" -or
        $downloads[0] -cne "https://api.nuget.org/v3-flatcontainer/microsoft.windows.sdk.cpp/10.0.2.0/microsoft.windows.sdk.cpp.10.0.2.0.nupkg" -or
        $updated.sha256 -ne (Get-FileHash -LiteralPath $sdkFixture -Algorithm SHA256).Hash -or
        $updated.files.Count -ne 2 -or $updated.files[0].path -cne $compiler -or $updated.files[1].path -cne $dll -or
        $updated.files[0].sha256 -cne "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad" -or
        $updated.files[1].sha256 -cne "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824") {
        throw "The updater did not pin the newest listed stable package and hash its actual files."
    }
    $before = (Get-FileHash -LiteralPath $manifest -Algorithm SHA256).Hash
    & (Join-Path $temporary "update.ps1")
    if ($downloads.Count -ne 1 -or (Get-FileHash -LiteralPath $manifest -Algorithm SHA256).Hash -ne $before) {
        throw "An up-to-date pin must not download an archive or rewrite the manifest."
    }
} finally { Remove-Item -LiteralPath $temporary -Recurse -Force }
Write-Output "Windows FXC update tests passed."

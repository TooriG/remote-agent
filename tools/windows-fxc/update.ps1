$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot "update-policy.ps1")

$manifest = Join-Path $PSScriptRoot "manifest.json"
$pin = Get-Content -LiteralPath $manifest -Raw | ConvertFrom-Json
$package = $pin.package.ToLowerInvariant()
$feed = "https://api.nuget.org/v3-flatcontainer/$package"
$versions = (Invoke-RestMethod -Uri "$feed/index.json").versions
$latest = Select-StableSdkVersion -Versions $versions -CurrentVersion $pin.version
while ($latest -ne $pin.version) {
    # The flat-container index includes withdrawn (unlisted) packages too.
    $metadata = Invoke-RestMethod -Uri "https://api.nuget.org/v3/registration5-semver1/$package/$latest.json"
    if ($metadata.listed -eq $true) { break }
    $versions = @($versions | Where-Object { $_ -ne $latest })
    $latest = Select-StableSdkVersion -Versions $versions -CurrentVersion $pin.version
}
if ($latest -eq $pin.version) {
    Write-Output "Windows FXC is current: $($pin.version)."
    return
}

$temporary = Join-Path ([IO.Path]::GetTempPath()) ("bex-sdk-update-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $temporary | Out-Null
try {
    $archive = Join-Path $temporary "sdk.nupkg"
    Invoke-WebRequest -UseBasicParsing -Uri "$feed/$latest/$package.$latest.nupkg" -OutFile $archive
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead($archive)
    try {
        $paths = Select-FxcPaths -EntryPaths @($zip.Entries.FullName)
        $files = @(foreach ($path in $paths) {
            $stream = $zip.GetEntry($path).Open()
            try { $hash = (Get-FileHash -InputStream $stream -Algorithm SHA256).Hash.ToLowerInvariant() }
            finally { $stream.Dispose() }
            [ordered]@{ path = $path; sha256 = $hash }
        })
    } finally { $zip.Dispose() }
    $updated = [ordered]@{
        package = $pin.package
        version = $latest
        sha256 = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
        files = $files
    } | ConvertTo-Json -Depth 4
    [IO.File]::WriteAllText($manifest, $updated + "`n", [Text.UTF8Encoding]::new($false))
    Write-Output "Updated Windows FXC from $($pin.version) to $latest."
} finally { Remove-Item -LiteralPath $temporary -Recurse -Force }

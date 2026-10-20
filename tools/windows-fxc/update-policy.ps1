function Select-StableSdkVersion([string[]]$Versions, [string]$CurrentVersion) {
    # NuGet omits a zero fourth component when normalizing versions.
    $stable = @($Versions | Where-Object { $_ -cmatch '^\d+\.\d+\.\d+(\.\d+)?$' } |
        Sort-Object { [version]$_ } -Descending)
    if ($stable.Count -eq 0) { throw "No stable Windows SDK versions found." }
    if ([version]$stable[0] -gt [version]$CurrentVersion) { return $stable[0] }
    return $CurrentVersion
}

function Select-FxcPaths([string[]]$EntryPaths) {
    $paths = @(foreach ($name in @("fxc.exe", "d3dcompiler_47.dll")) {
        $pattern = '^c/bin/\d+\.\d+\.\d+\.\d+/x64/' + [regex]::Escape($name) + '$'
        $candidates = @($EntryPaths | Where-Object { $_ -cmatch $pattern })
        if ($candidates.Count -ne 1) { throw "Expected exactly one x64 SDK entry for $name." }
        $candidates[0]
    })
    if (($paths[0] -replace '/[^/]+$', '') -cne ($paths[1] -replace '/[^/]+$', '')) {
        throw "FXC and its DLL must come from the same SDK directory."
    }
    return $paths
}

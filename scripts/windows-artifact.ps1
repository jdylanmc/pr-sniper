$ErrorActionPreference = 'Stop'
$repository = Split-Path -Parent $PSScriptRoot
$target = if ($env:CARGO_TARGET_DIR) {
    if ([System.IO.Path]::IsPathRooted($env:CARGO_TARGET_DIR)) {
        [System.IO.Path]::GetFullPath($env:CARGO_TARGET_DIR)
    } else {
        [System.IO.Path]::GetFullPath((Join-Path $repository $env:CARGO_TARGET_DIR))
    }
} else {
    Join-Path $repository 'src-tauri\target'
}
$executable = Join-Path $target 'release\pr-sniper.exe'
$bytes = [System.IO.File]::ReadAllBytes($executable)
if ($bytes.Length -lt 64 -or $bytes[0] -ne 0x4d -or $bytes[1] -ne 0x5a) {
    throw 'Windows artifact is not a PE executable.'
}
$pe = [BitConverter]::ToUInt32($bytes, 0x3c)
if ($pe -gt $bytes.Length - 94 -or [BitConverter]::ToUInt32($bytes, $pe) -ne 0x4550) {
    throw 'Windows artifact has an invalid PE header.'
}
if ([BitConverter]::ToUInt16($bytes, $pe + 4) -ne 0x8664 -or
    [BitConverter]::ToUInt16($bytes, $pe + 24) -ne 0x20b -or
    [BitConverter]::ToUInt16($bytes, $pe + 92) -ne 2) {
    throw 'Windows artifact must be an x64 GUI executable, not a console app.'
}
if ((Get-AuthenticodeSignature -LiteralPath $executable).Status -ne 'NotSigned') {
    throw 'Ordinary Windows CI must produce an unsigned application candidate.'
}
$commit = git -C $repository rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $commit -notmatch '^[0-9a-f]{40}$') {
    throw 'Cannot identify the artifact source commit.'
}
if ($env:GITHUB_SHA -and $commit -ne $env:GITHUB_SHA) {
    throw 'Artifact source differs from the checked CI commit.'
}
$changes = git -C $repository status --porcelain
if ($LASTEXITCODE -ne 0 -or $changes) {
    throw 'Commit source changes before identifying a Windows artifact.'
}
$version = (Get-Content (Join-Path $repository 'package.json') -Raw | ConvertFrom-Json).version
$hash = (Get-FileHash -LiteralPath $executable -Algorithm SHA256).Hash.ToLowerInvariant()
$destination = Join-Path $target 'windows-artifact'
New-Item -ItemType Directory -Path $destination -ErrorAction Stop | Out-Null
Copy-Item -LiteralPath $executable -Destination (Join-Path $destination 'pr-sniper.exe')
if ((Get-FileHash (Join-Path $destination 'pr-sniper.exe') -Algorithm SHA256).Hash.ToLowerInvariant() -ne $hash) {
    throw 'Staged Windows executable differs from the inspected build.'
}
[ordered]@{
    version = $version
    commit = $commit
    target = 'x86_64-pc-windows-msvc'
    sha256 = $hash
    signed = $false
} | ConvertTo-Json | Set-Content (Join-Path $destination 'build.json') -Encoding utf8
"$hash  pr-sniper.exe" | Set-Content (Join-Path $destination 'SHA256SUMS') -Encoding ascii
"Windows artifact: version=$version commit=$commit sha256=$hash (unsigned; native launch proof separate)"

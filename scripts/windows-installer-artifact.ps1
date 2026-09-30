param([string] $Destination)
$ErrorActionPreference = 'Stop'
$repository = Split-Path -Parent $PSScriptRoot
$target = if ($env:CARGO_TARGET_DIR) {
    if ([IO.Path]::IsPathRooted($env:CARGO_TARGET_DIR)) { [IO.Path]::GetFullPath($env:CARGO_TARGET_DIR) }
    else { [IO.Path]::GetFullPath((Join-Path $repository $env:CARGO_TARGET_DIR)) }
} else {
    Join-Path $repository 'src-tauri\target'
}
if (-not $Destination) { $Destination = Join-Path $target 'windows-installer-artifact' }
$source = Get-Content (Join-Path $target 'windows-artifact\build.json') -Raw | ConvertFrom-Json
$commit = git -C $repository rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $source.commit -cne $commit -or
    ($env:GITHUB_SHA -and $commit -cne $env:GITHUB_SHA)) { throw 'Installer source commit mismatch.' }
$changes = git -C $repository status --porcelain
if ($LASTEXITCODE -ne 0 -or $changes) { throw 'Installer provenance requires clean committed source.' }
& python -B (Join-Path $PSScriptRoot 'release.py') check
if ($LASTEXITCODE -ne 0) { throw 'Application version/identity check failed.' }
if ($source.target -cne 'x86_64-pc-windows-msvc' -or $source.signed -ne $false) {
    throw 'Expected the unsigned, verified x64 GUI application artifact.'
}
$app = Join-Path $target 'release\pr-sniper.exe'
if ((Get-FileHash $app -Algorithm SHA256).Hash -ine $source.sha256) {
    throw 'Application changed after standalone artifact verification.'
}
$version = (Get-Content (Join-Path $repository 'package.json') -Raw | ConvertFrom-Json).version
if ($version -cne $source.version) { throw 'Application artifact version mismatch.' }
$installer = Join-Path $target "release\bundle\nsis\PR Sniper_${version}_x64-setup.exe"
$info = [Diagnostics.FileVersionInfo]::GetVersionInfo($installer)
if ($info.ProductVersion -cne $version -or $info.ProductName -cne 'PR Sniper') {
    throw 'Installer version resource does not match the application.'
}
if ((Get-AuthenticodeSignature $installer).Status -ne 'NotSigned') {
    throw 'This lane stages unsigned CI candidates only, never signed releases.'
}
$filename = "pr-sniper-$version-x64-setup.exe"
$hash = (Get-FileHash $installer -Algorithm SHA256).Hash.ToLowerInvariant()
New-Item -ItemType Directory $Destination -ErrorAction Stop | Out-Null
Copy-Item -LiteralPath $installer -Destination (Join-Path $Destination $filename)
if ((Get-FileHash (Join-Path $Destination $filename)).Hash -ine $hash) { throw 'Staged installer changed.' }
[ordered]@{
    schema = 1
    version = $version
    commit = $commit
    target = $source.target
    filename = $filename
    sha256 = $hash
    application_sha256 = $source.sha256
    bundle_id = 'com.jdylanmc.pr-sniper'
    distribution = 'unsigned-ci-candidate-not-a-public-release'
    signed = $false
} | ConvertTo-Json | Set-Content (Join-Path $Destination 'installer.json') -Encoding utf8
"$hash  $filename" | Set-Content (Join-Path $Destination 'SHA256SUMS') -Encoding ascii
"Unsigned installer candidate: version=$version commit=$commit sha256=$hash"

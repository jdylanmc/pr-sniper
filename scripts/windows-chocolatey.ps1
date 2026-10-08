param(
    [Parameter(Mandatory)][ValidateSet('LocalTest', 'Public', 'PublicUnsigned')][string] $Mode,
    [Parameter(Mandatory)][string] $Installer,
    [Parameter(Mandatory)][ValidatePattern('^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$')][string] $Version,
    [Parameter(Mandatory)][ValidatePattern('^[a-fA-F0-9]{64}$')][string] $Sha256,
    [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{40}$')][string] $Commit,
    [Parameter(Mandatory)][string] $Destination,
    [string] $Url,
    [string] $ExpectedThumbprint,
    [string] $ExpectedSubject,
    [string] $LicenseUrl
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-signature.ps1')
$Installer = (Resolve-Path -LiteralPath $Installer).Path
if ((Get-FileHash $Installer -Algorithm SHA256).Hash -ine $Sha256) { throw 'Installer checksum mismatch.' }
$info = [Diagnostics.FileVersionInfo]::GetVersionInfo($Installer)
if ($info.ProductVersion -cne $Version -or $info.ProductName -cne 'PR Sniper') {
    throw 'Installer identity/version mismatch.'
}
$id = 'pr-sniper-localtest'
$packageVersion = "$Version-localtest"
$public = $Mode -in @('Public', 'PublicUnsigned')
if ($public) {
    $expectedUrl = "https://github.com/jdylanmc/pr-sniper/releases/download/v$Version/pr-sniper-$Version-x64-setup.exe"
    $expectedLicense = "https://github.com/jdylanmc/pr-sniper/blob/$Commit/LICENSE"
    if ($Url -cne $expectedUrl -or $LicenseUrl -cne $expectedLicense) {
        throw 'Public packaging requires the immutable project URL and MIT license URL at the exact release commit.'
    }
    if ($Mode -eq 'Public') {
        if (-not $ExpectedThumbprint -or -not $ExpectedSubject) {
            throw 'Signed public packaging requires the trusted publisher identity.'
        }
        Assert-TrustedWindowsSignature $Installer $ExpectedThumbprint $ExpectedSubject -RequireSignTool
    } else {
        if ($ExpectedThumbprint -or $ExpectedSubject) {
            throw 'Unsigned public packaging must not claim an Authenticode publisher.'
        }
        Assert-UnsignedWindowsArtifact $Installer
    }
    $gate = & python -B (Join-Path $PSScriptRoot 'release.py') check-windows-release
    if ($LASTEXITCODE -ne 0) { throw 'Exact-tag Windows/macOS release preflight failed.' }
    $gate = $gate | ConvertFrom-Json
    if ($gate.sha -cne $Commit -or $gate.version -cne $Version) { throw 'Release provenance mismatch.' }
    $id = 'pr-sniper'
    $packageVersion = $Version
} elseif ((Get-AuthenticodeSignature $Installer).Status -ne 'NotSigned') {
    throw 'The local-test variant accepts unsigned candidates only.'
}
New-Item -ItemType Directory -Path $Destination -ErrorAction Stop | Out-Null
$tools = New-Item -ItemType Directory -Path (Join-Path $Destination 'tools')
if ($Mode -eq 'LocalTest') {
    Copy-Item $Installer (Join-Path $tools.FullName 'installer.exe')
} else {
    # Fetch the public bytes without provider credentials. Never package local
    # signer output that differs from the already-published immutable artifact.
    $download = Join-Path $tools.FullName 'public-verification.exe'
    try {
        Invoke-WebRequest -Uri $Url -OutFile $download
        if ((Get-FileHash $download -Algorithm SHA256).Hash -ine $Sha256) {
            throw 'Public release bytes differ from the expected installer.'
        }
        if ($Mode -eq 'Public') {
            Assert-TrustedWindowsSignature $download $ExpectedThumbprint $ExpectedSubject -RequireSignTool
        } else {
            Assert-UnsignedWindowsArtifact $download
        }
    } finally {
        if (Test-Path $download) { Remove-Item -LiteralPath $download }
    }
}
$metadata = [ordered]@{
    mode = $Mode
    package_id = $id
    version = $Version
    commit = $Commit
    target = 'x86_64-pc-windows-msvc'
    sha256 = $Sha256.ToLowerInvariant()
    url = $Url
    signer_thumbprint = $ExpectedThumbprint
    signer_subject = $ExpectedSubject
}
$metadata | ConvertTo-Json | Set-Content (Join-Path $tools.FullName 'installer.json') -Encoding utf8
Copy-Item (Join-Path $PSScriptRoot 'windows-signature.ps1') $tools.FullName
Copy-Item (Join-Path $PSScriptRoot 'windows-webview2.ps1') $tools.FullName
Copy-Item (Join-Path $PSScriptRoot '..\packaging\chocolatey\*.ps1') $tools.FullName
'' | Set-Content (Join-Path $tools.FullName 'installer.exe.ignore') -Encoding ascii
$license = if ($public) {
    '<licenseUrl>' + [Security.SecurityElement]::Escape($LicenseUrl) + '</licenseUrl>'
} else { '' }
$description = 'PR Sniper monitors configured repositories and prepares human-owned pull request review work. Windows 10/11 x64 and Microsoft Edge WebView2 Evergreen Runtime are required. Installs for the invoking user; does not start the app or enable login, notification or review automation. Settings and credentials are preserved. Copilot use requires a separate eligible account/subscription.'
if ($Mode -eq 'LocalTest') {
    $description = 'UNSIGNED TEST ONLY: hosted CI or explicitly authorized exact-hash local testing. NEVER PUBLISH. No project license or redistribution grant is asserted. ' + $description
} elseif ($Mode -eq 'PublicUnsigned') {
    $description = 'UNSIGNED PUBLIC RELEASE: No Authenticode publisher identity. The installer is verified against its pinned SHA-256, not a signing certificate. Windows security policies may warn or block it; do not disable security protections. ' + $description
}
if ($public) {
    $description += ' Chocolatey provisions the shared WebView2 runtime as a dependency; that dependency may require elevation. Quit PR Sniper before upgrading.'
}
$dependencies = if ($public) {
    '<dependencies><dependency id="webview2-runtime" version="[154.0.4258.62,)" /></dependencies>'
} else { '' }
@"
<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://schemas.microsoft.com/packaging/2015/06/nuspec.xsd">
  <metadata>
    <id>$id</id>
    <version>$packageVersion</version>
    <title>PR Sniper</title>
    <authors>Dylan McCurry</authors>
    <projectUrl>https://github.com/jdylanmc/pr-sniper</projectUrl>
    <packageSourceUrl>https://github.com/jdylanmc/pr-sniper/tree/$Commit/packaging/chocolatey</packageSourceUrl>
    <bugTrackerUrl>https://github.com/jdylanmc/pr-sniper/issues</bugTrackerUrl>
    $license
    $dependencies
    <requireLicenseAcceptance>false</requireLicenseAcceptance>
    <summary>Human-owned pull request review from the Windows system tray.</summary>
    <description>$description</description>
    <tags>pull-request review developer github</tags>
  </metadata>
  <files><file src="tools\**" target="tools" /></files>
</package>
"@ | Set-Content (Join-Path $Destination "$id.nuspec") -Encoding utf8
"Package source: $id $packageVersion ($Mode); generation is not publication."

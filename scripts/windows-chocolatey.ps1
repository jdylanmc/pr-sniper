param(
    [Parameter(Mandatory)][ValidateSet('LocalTest', 'Public')][string] $Mode,
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
if ($Mode -eq 'Public') {
    $expectedUrl = "https://github.com/jdylanmc/pr-sniper/releases/download/v$Version/pr-sniper-$Version-x64-setup.exe"
    if ($Url -cne $expectedUrl -or $LicenseUrl -notmatch '^https://[^/]+/' -or
        -not $ExpectedThumbprint -or -not $ExpectedSubject) {
        throw 'Public packaging requires the immutable project URL, approved license URL and trusted publisher identity.'
    }
    Assert-TrustedWindowsSignature $Installer $ExpectedThumbprint $ExpectedSubject -RequireSignTool
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
        Assert-TrustedWindowsSignature $download $ExpectedThumbprint $ExpectedSubject -RequireSignTool
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
Copy-Item (Join-Path $PSScriptRoot '..\packaging\chocolatey\*.ps1') $tools.FullName
'' | Set-Content (Join-Path $tools.FullName 'installer.exe.ignore') -Encoding ascii
$license = if ($Mode -eq 'Public') {
    '<licenseUrl>' + [Security.SecurityElement]::Escape($LicenseUrl) + '</licenseUrl>'
} else { '' }
$description = 'PR Sniper monitors configured repositories and prepares human-owned pull request review work. Windows x64 and Microsoft Edge WebView2 Evergreen Runtime are required. Installs for the invoking user; does not start the app or enable login, notification or review automation. Settings and credentials are preserved.'
if ($Mode -eq 'LocalTest') {
    $description = 'UNSIGNED HOSTED-CI TEST ONLY. NEVER PUBLISH. No project license or redistribution grant is asserted. ' + $description
}
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
    <requireLicenseAcceptance>false</requireLicenseAcceptance>
    <summary>Human-owned pull request review from the Windows system tray.</summary>
    <description>$description</description>
    <tags>pull-request review developer github</tags>
  </metadata>
  <files><file src="tools\**" target="tools" /></files>
</package>
"@ | Set-Content (Join-Path $Destination "$id.nuspec") -Encoding utf8
"Package source: $id $packageVersion ($Mode); generation is not publication."

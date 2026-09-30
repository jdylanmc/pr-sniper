$ErrorActionPreference = 'Stop'
$tools = Split-Path -Parent $MyInvocation.MyCommand.Definition
$metadata = Get-Content (Join-Path $tools 'installer.json') -Raw | ConvertFrom-Json
. (Join-Path $tools 'windows-signature.ps1')
if ($env:PROCESSOR_ARCHITECTURE -ne 'AMD64' -or $metadata.target -cne 'x86_64-pc-windows-msvc') {
    throw 'This package supports only native Windows x64.'
}
$installer = Join-Path $tools 'installer.exe'
if ($metadata.mode -eq 'LocalTest') {
    if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or
        $env:PR_SNIPER_PACKAGING_ACCEPTANCE -ne '1') {
        throw 'Unsigned package installation is restricted to the disposable hosted-CI acceptance job. Never publish it.'
    }
} elseif ($metadata.mode -eq 'Public') {
    $url = "https://github.com/jdylanmc/pr-sniper/releases/download/v$($metadata.version)/pr-sniper-$($metadata.version)-x64-setup.exe"
    if ($metadata.url -cne $url) { throw 'Unexpected public installer URL.' }
    Get-ChocolateyWebFile -PackageName 'pr-sniper' -FileFullPath $installer -Url64bit $url `
        -Checksum64 $metadata.sha256 -ChecksumType64 'sha256' | Out-Null
    Assert-TrustedWindowsSignature $installer $metadata.signer_thumbprint $metadata.signer_subject
} else { throw 'Unknown package distribution mode.' }
# Retain the pin even if the caller disables Chocolatey's own checksum feature.
if ((Get-FileHash $installer -Algorithm SHA256).Hash -ine $metadata.sha256) { throw 'Installer checksum mismatch.' }
$info = [Diagnostics.FileVersionInfo]::GetVersionInfo($installer)
if ($info.ProductVersion -cne $metadata.version -or $info.ProductName -cne 'PR Sniper') {
    throw 'Installer identity/version mismatch.'
}
Start-ChocolateyProcessAsAdmin -ExeToRun $installer -Statements '/S' -Elevated:$false -ValidExitCodes @(0)
$directory = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'PR Sniper'
$app = Join-Path $directory 'pr-sniper.exe'
if ([Diagnostics.FileVersionInfo]::GetVersionInfo($app).ProductVersion -cne $metadata.version) {
    throw 'Installed application version differs from the package.'
}
$uninstaller = Join-Path $directory 'uninstall.exe'
[ordered]@{
    directory = $directory
    uninstaller_sha256 = (Get-FileHash $uninstaller -Algorithm SHA256).Hash
    version = $metadata.version
} | ConvertTo-Json | Set-Content (Join-Path $tools 'installation.json') -Encoding utf8

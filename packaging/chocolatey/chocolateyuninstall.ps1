$ErrorActionPreference = 'Stop'
$tools = Split-Path -Parent $MyInvocation.MyCommand.Definition
. (Join-Path $tools 'windows-signature.ps1')
$receipt = Get-Content (Join-Path $tools 'installation.json') -Raw | ConvertFrom-Json
$directory = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'PR Sniper'
$uninstaller = Join-Path $directory 'uninstall.exe'
if ($receipt.directory -cne $directory -or
    (Get-FileHash $uninstaller -Algorithm SHA256).Hash -ine $receipt.uninstaller_sha256) {
    throw 'The exact package-owned uninstaller is unavailable or has changed.'
}
$base = [Microsoft.Win32.RegistryKey]::OpenBaseKey('CurrentUser', 'Registry64')
$key = $base.OpenSubKey('Software\Microsoft\Windows\CurrentVersion\Uninstall\PR Sniper')
try {
    if (-not $key -or $key.GetValue('DisplayVersion') -cne $receipt.version -or
        $key.GetValue('PRSniperInstaller') -cne "com.jdylanmc.pr-sniper|$directory\pr-sniper.exe" -or
        $key.GetValue('InstallLocation') -cne $directory -or
        $key.GetValue('UninstallString') -cne "`"$uninstaller`"") {
        throw 'Installer registration no longer belongs to this exact package installation.'
    }
} finally {
    if ($key) { $key.Dispose() }
    $base.Dispose()
}
# NSIS _?= must be last, without quotes; it prevents a detached uninstaller
# copy so Chocolatey observes the actual result before cleaning the owned file.
Start-ChocolateyProcessAsAdmin -ExeToRun $uninstaller -Statements "/S _?=$directory" `
    -Elevated:$false -ValidExitCodes @(0)
if (Test-Path (Join-Path $directory 'pr-sniper.exe')) { throw 'Application removal did not complete.' }
if (Test-Path $uninstaller) {
    if ((Get-FileHash $uninstaller).Hash -ine $receipt.uninstaller_sha256) { throw 'Uninstaller changed during removal.' }
    Remove-Item -LiteralPath $uninstaller
}
if ((Test-Path $directory) -and -not (Get-ChildItem -LiteralPath $directory -Force)) {
    Remove-Item -LiteralPath $directory
}

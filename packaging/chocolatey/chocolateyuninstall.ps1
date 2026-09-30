$ErrorActionPreference = 'Stop'
$tools = Split-Path -Parent $MyInvocation.MyCommand.Definition
. (Join-Path $tools 'windows-signature.ps1')
. (Join-Path $tools 'removal-state.ps1')
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
$sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$lifecycle = [Threading.Mutex]::new($false, "Global\com.jdylanmc.pr-sniper.installer.$sid")
$locked = $false
try {
try { $locked = $lifecycle.WaitOne(0) }
catch [Threading.AbandonedMutexException] { $locked = $true }
if (-not $locked) { throw 'Another installer owns post-uninstall cleanup; receipt retained.' }
$base = [Microsoft.Win32.RegistryKey]::OpenBaseKey('CurrentUser', 'Registry64')
$key = $base.OpenSubKey('Software\Microsoft\Windows\CurrentVersion\Uninstall\PR Sniper')
try { $values = if ($key) { @($key.GetValueNames()) } else { @() } }
finally {
    if ($key) { $key.Dispose() }
    $base.Dispose()
}
$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'PR Sniper.lnk'
$shortcutExists = Test-Path -LiteralPath $shortcut
$shortcutTarget = if ($shortcutExists) { (New-Object -ComObject WScript.Shell).CreateShortcut($shortcut).TargetPath } else { '' }
Assert-PrSniperRemovalState -AppExists (Test-Path (Join-Path $directory 'pr-sniper.exe')) `
    -TransactionExists (Test-Path (Join-Path $directory '.pr-sniper-transaction')) `
    -RegistryValueNames $values -ShortcutExists $shortcutExists -ShortcutTarget $shortcutTarget `
    -ExpectedApp (Join-Path $directory 'pr-sniper.exe')
if (Test-Path $uninstaller) {
    if ((Get-FileHash $uninstaller).Hash -ine $receipt.uninstaller_sha256) { throw 'Uninstaller changed during removal.' }
    Remove-Item -LiteralPath $uninstaller
}
if ((Test-Path $directory) -and -not (Get-ChildItem -LiteralPath $directory -Force)) {
    Remove-Item -LiteralPath $directory
}
} finally {
    if ($locked) { $lifecycle.ReleaseMutex() }
    $lifecycle.Dispose()
}

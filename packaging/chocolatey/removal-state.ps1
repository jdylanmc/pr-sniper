function Assert-PrSniperRemovalState {
    param(
        [bool] $AppExists,
        [bool] $TransactionExists,
        [AllowEmptyCollection()][string[]] $RegistryValueNames,
        [bool] $ShortcutExists,
        [string] $ShortcutTarget,
        [string] $ExpectedApp
    )
    if ($AppExists) { throw 'Application removal did not complete; recovery executable/receipt retained.' }
    if ($TransactionExists) { throw 'Installer transaction remains; recovery executable/receipt retained.' }
    $ownedValues = @('PRSniperInstaller','DisplayName','DisplayVersion','DisplayIcon','Publisher',
        'InstallLocation','UninstallString','QuietUninstallString','NoModify','NoRepair')
    if (@($RegistryValueNames | Where-Object { $_ -in $ownedValues }).Count) {
        throw 'Owned installer registration remains; recovery executable/receipt retained.'
    }
    if ($ShortcutExists -and (-not $ShortcutTarget -or $ShortcutTarget -ieq $ExpectedApp)) {
        throw 'Owned shortcut remains or cannot be resolved; recovery executable/receipt retained.'
    }
}

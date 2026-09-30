function Assert-PrSniperRemovalState {
    param(
        [bool] $AppExists,
        [bool] $TransactionExists,
        [bool] $RegistryKeyExists = $false,
        [int] $RegistrySubKeyCount = 0,
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
    if ($RegistryKeyExists -and $RegistrySubKeyCount -eq 0 -and
        @($RegistryValueNames | Where-Object { $null -ne $_ }).Count -eq 0) {
        throw 'Empty owned installer key remains; recovery executable/receipt retained.'
    }
    if ($ShortcutExists -and (-not $ShortcutTarget -or $ShortcutTarget -ieq $ExpectedApp)) {
        throw 'Owned shortcut remains or cannot be resolved; recovery executable/receipt retained.'
    }

}

function Assert-PrSniperRemovalReceipt($Completion, $Receipt, [string] $ReceiptHash) {
    if ($Receipt.uninstaller_sha256 -notmatch '^[a-fA-F0-9]{64}$' -or
        [string]::IsNullOrEmpty($Receipt.version) -or
        $Completion.schema -ne 1 -or $Completion.phase -cne 'native-removal-complete' -or
        $Completion.installation_receipt_sha256 -ine $ReceiptHash -or
        $Completion.directory -cne $Receipt.directory -or $Completion.version -cne $Receipt.version -or
        $Completion.uninstaller_sha256 -ine $Receipt.uninstaller_sha256) {
        throw 'Completed native removal is not evidenced by this exact installation receipt.'
    }
}

function Test-PrSniperUninstallerCleanupRequired(
    $Completion, $Receipt, [string] $ReceiptHash, [bool] $UninstallerExists, [string] $UninstallerHash
) {
    Assert-PrSniperRemovalReceipt $Completion $Receipt $ReceiptHash
    if (-not $UninstallerExists) { return $false }
    if ($UninstallerHash -ine $Receipt.uninstaller_sha256) { throw 'Uninstaller changed before cleanup.' }
    return $true
}

function Get-PrSniperUninstallerFileState([string] $Path) {
    try { $attributes = [IO.File]::GetAttributes($Path) }
    catch [IO.FileNotFoundException], [IO.DirectoryNotFoundException] {
        return @{ exists = $false; sha256 = '' }
    }
    if ($attributes -band [IO.FileAttributes]::Directory) { throw 'Uninstaller path is not a regular file.' }
    return @{ exists = $true; sha256 = (Get-FileHash -LiteralPath $Path -Algorithm SHA256 -ErrorAction Stop).Hash }
}

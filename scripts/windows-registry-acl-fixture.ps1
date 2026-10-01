function Test-PrSniperDaclReadback([byte[]] $Expected, [byte[]] $Observed) {
    if ($Expected.Length -lt 20 -or $Expected.Length -ne $Observed.Length) { return $false }
    $expectedEncoding = [Convert]::ToBase64String($Expected)
    if ([Convert]::ToBase64String($Observed) -ceq $expectedEncoding) { return $true }
    # Admit only the observed kernel addition of SE_DACL_AUTO_INHERITED (0x0400).
    # ACE bytes/order, protection, inheritance requests and every other byte stay exact.
    $normalized = [byte[]]$Expected.Clone()
    $normalized[3] = $normalized[3] -bor 0x04
    return [Convert]::ToBase64String($Observed) -ceq [Convert]::ToBase64String($normalized)
}

function Invoke-PrSniperRegistryAclDenial {
    param(
        [Parameter(Mandatory)] $RestoreKey,
        [Parameter(Mandatory)][Security.Principal.SecurityIdentifier] $Identity,
        [Parameter(Mandatory)][ValidateSet('SetValue','Delete')][string] $Right,
        [Parameter(Mandatory)][scriptblock] $Exercise
    )
    $section = [Security.AccessControl.AccessControlSections]::Access
    $original = $RestoreKey.GetAccessControl($section).GetSecurityDescriptorBinaryForm()
    $denied = [Security.AccessControl.RegistrySecurity]::new()
    $denied.SetSecurityDescriptorBinaryForm($original, $section)
    $denied.AddAccessRule([Security.AccessControl.RegistryAccessRule]::new(
        $Identity, $Right, 'None', 'None', 'Deny'))
    $expectedDenial = $denied.GetSecurityDescriptorBinaryForm()
    $failure = $null
    try {
        $RestoreKey.SetAccessControl($denied)
        if (-not (Test-PrSniperDaclReadback $expectedDenial $RestoreKey.GetAccessControl($section).GetSecurityDescriptorBinaryForm())) {
            throw 'Owned registry denial permissions/inheritance did not persist; fixture was not exercised.'
        }
        & $Exercise
    } catch { $failure = $_ }
    finally {
        try {
            # A freshly populated descriptor marks AccessRulesModified. Reusing
            # an unchanged GetAccessControl result can make .NET Persist a no-op.
            $restore = [Security.AccessControl.RegistrySecurity]::new()
            $restore.SetSecurityDescriptorBinaryForm($original, $section)
            $RestoreKey.SetAccessControl($restore)
            if (-not (Test-PrSniperDaclReadback $original $RestoreKey.GetAccessControl($section).GetSecurityDescriptorBinaryForm())) {
                throw 'Owned registry DACL restoration readback differs from the original.'
            }
        } catch {
            if ($failure) {
                Write-Warning "Additional owned registry ACL restoration failure: $($_.Exception.Message). Preserving original fixture failure."
            } else { $failure = $_ }
        }
    }
    if ($failure) { throw $failure }
}

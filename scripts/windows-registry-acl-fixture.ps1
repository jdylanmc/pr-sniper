function Invoke-PrSniperRegistryAclDenial {
    param(
        [Parameter(Mandatory)] $RestoreKey,
        [Parameter(Mandatory)][Security.Principal.SecurityIdentifier] $Identity,
        [Parameter(Mandatory)][ValidateSet('SetValue','Delete')][string] $Right,
        [Parameter(Mandatory)][scriptblock] $Exercise
    )
    $section = [Security.AccessControl.AccessControlSections]::Access
    $original = $RestoreKey.GetAccessControl($section).GetSecurityDescriptorBinaryForm()
    $originalEncoding = [Convert]::ToBase64String($original)
    $denied = [Security.AccessControl.RegistrySecurity]::new()
    $denied.SetSecurityDescriptorBinaryForm($original, $section)
    $denied.AddAccessRule([Security.AccessControl.RegistryAccessRule]::new(
        $Identity, $Right, 'None', 'None', 'Deny'))
    $expectedDenial = [Convert]::ToBase64String($denied.GetSecurityDescriptorBinaryForm())
    $failure = $null
    try {
        $RestoreKey.SetAccessControl($denied)
        if ([Convert]::ToBase64String($RestoreKey.GetAccessControl($section).GetSecurityDescriptorBinaryForm()) -cne $expectedDenial) {
            throw 'Owned registry denial did not persist exactly; fixture was not exercised.'
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
            if ([Convert]::ToBase64String($RestoreKey.GetAccessControl($section).GetSecurityDescriptorBinaryForm()) -cne $originalEncoding) {
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

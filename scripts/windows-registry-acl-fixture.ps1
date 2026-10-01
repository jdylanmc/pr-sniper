function Test-PrSniperDaclReadback([byte[]] $Expected, [byte[]] $Observed) {
    if ($Expected.Length -lt 20 -or $Observed.Length -lt 20) { return $false }
    try {
        $expectedDescriptor = [Security.AccessControl.RawSecurityDescriptor]::new($Expected, 0)
        $observedDescriptor = [Security.AccessControl.RawSecurityDescriptor]::new($Observed, 0)
    } catch [ArgumentException] { return $false }
    # Access-only reads can change owner/group presence and offsets. Compare the
    # raw DACL, never a reconstructed CommonAcl that could canonicalize its ACEs.
    $flags = [Security.AccessControl.ControlFlags]
    $daclMask = $flags::DiscretionaryAclPresent -bor $flags::DiscretionaryAclDefaulted -bor
        $flags::DiscretionaryAclUntrusted -bor $flags::ServerSecurity -bor
        $flags::DiscretionaryAclAutoInheritRequired -bor $flags::DiscretionaryAclAutoInherited -bor
        $flags::DiscretionaryAclProtected
    $expectedFlags = $expectedDescriptor.ControlFlags -band $daclMask
    $observedFlags = $observedDescriptor.ControlFlags -band $daclMask
    if (-not ($expectedFlags -band $flags::DiscretionaryAclPresent) -or
        -not ($observedFlags -band $flags::DiscretionaryAclPresent) -or
        $null -eq $expectedDescriptor.DiscretionaryAcl -or $null -eq $observedDescriptor.DiscretionaryAcl) {
        return $false
    }
    if ($observedFlags -ne $expectedFlags -and
        $observedFlags -ne ($expectedFlags -bor $flags::DiscretionaryAclAutoInherited)) {
        return $false
    }
    $expectedAcl = New-Object byte[] $expectedDescriptor.DiscretionaryAcl.BinaryLength
    $observedAcl = New-Object byte[] $observedDescriptor.DiscretionaryAcl.BinaryLength
    $expectedDescriptor.DiscretionaryAcl.GetBinaryForm($expectedAcl, 0)
    $observedDescriptor.DiscretionaryAcl.GetBinaryForm($observedAcl, 0)
    return [Convert]::ToBase64String($expectedAcl) -ceq [Convert]::ToBase64String($observedAcl)
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

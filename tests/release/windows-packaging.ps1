$ErrorActionPreference = 'Stop'
$repository = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
. (Join-Path $repository 'scripts\windows-signature.ps1')
$fixture = Join-Path $repository ('src-tauri\target\packaging-tests-' + [guid]::NewGuid())
New-Item -ItemType Directory $fixture | Out-Null
$originalTemp = $env:TEMP
$originalTmp = $env:TMP
$originalActions = $env:GITHUB_ACTIONS
$originalLocalConsent = $env:PR_SNIPER_UNSIGNED_LOCAL_TEST_SHA256
$originalDiagnostics = $env:PR_SNIPER_INSTALLER_DIAGNOSTICS
$env:TEMP = $fixture
$env:TMP = $fixture
$env:GITHUB_ACTIONS = ''
$env:PR_SNIPER_UNSIGNED_LOCAL_TEST_SHA256 = ''
$script:count = 0
function Check([bool] $Condition, [string] $Message) {
    if (-not $Condition) { throw $Message }
    $script:count++
}
function Reject([scriptblock] $Operation, [string] $Pattern) {
    $errorSeen = $null
    try { & $Operation | Out-Null } catch { $errorSeen = $_.Exception.Message }
    Check ($null -ne $errorSeen -and $errorSeen -match $Pattern) "Expected rejection '$Pattern', got '$errorSeen'."
}
try {
    . (Join-Path $repository 'scripts\windows-host-readiness.ps1')
    . (Join-Path $repository 'packaging\chocolatey\removal-state.ps1')
    . (Join-Path $repository 'scripts\windows-installer-payload.ps1')
    . (Join-Path $repository 'scripts\windows-registry-acl-fixture.ps1')
    . (Join-Path $repository 'scripts\windows-removal-diagnostics.ps1')
    . (Join-Path $repository 'scripts\windows-choco-test.ps1')
    $env:PR_SNIPER_INSTALLER_DIAGNOSTICS = Join-Path $fixture 'choco-logs'
    New-Item -ItemType Directory $env:PR_SNIPER_INSTALLER_DIAGNOSTICS | Out-Null
    $script:chocoExit = 0
    $script:chocoThrow = $false
    function choco {
        if ($script:chocoThrow) { throw 'Fixture command could not start.' }
        $script:chocoArguments = @($args)
        Write-Output 'Fixture Chocolatey output, not an actual package operation.'
        $global:LASTEXITCODE = $script:chocoExit
    }
    $result = Invoke-PrSniperChocoTest -Name 'fixture-success' -Arguments @('install', 'fixture-only')
    Check ($result.exit_code -eq 0 -and (Get-Content $result.log -Raw) -match 'Fixture Chocolatey output') 'Successful commands preserve their raw output.'
    Check ($script:chocoArguments -contains '--execution-timeout=180') 'Chocolatey script execution remains bounded.'
    $script:chocoExit = 2
    $expected = Invoke-PrSniperChocoTest -Name 'fixture-expected-failure' -Arguments @('uninstall','fixture-only') -ExpectFailure -TimeoutSeconds 60
    Check ($expected.exit_code -eq 2 -and $expected.log -ne $result.log) 'Expected failure is explicit and uses a unique retained log.'
    Check ($script:chocoArguments -contains '--execution-timeout=60') 'Fault-specific command timeout is retained.'
    Reject { Invoke-PrSniperChocoTest -Name 'fixture-unexpected-failure' -Arguments @('install','fixture-only') } 'expected zero, got 2'
    $script:chocoExit = 0
    Reject { Invoke-PrSniperChocoTest -Name 'fixture-unexpected-success' -Arguments @('uninstall','fixture-only') -ExpectFailure } 'expected nonzero.*got 0'
    $script:chocoThrow = $true
    Reject { Invoke-PrSniperChocoTest -Name 'fixture-launch-failure' -Arguments @('install','fixture-only') -ExpectFailure } 'could not start'
    $env:PR_SNIPER_INSTALLER_DIAGNOSTICS = ''
    Reject { Invoke-PrSniperChocoTest -Name 'fixture-no-logs' -Arguments @('install','fixture-only') } 'owned diagnostics directory'
    Remove-Item Function:\choco
    $env:PR_SNIPER_INSTALLER_DIAGNOSTICS = $originalDiagnostics
    function New-MemoryAclKey([switch] $Unprotected, [switch] $NormalizeNative) {
        $descriptor = [Security.AccessControl.RegistrySecurity]::new()
        $prefix = if ($Unprotected) { 'D:' } else { 'D:P' }
        $descriptor.SetSecurityDescriptorSddlForm($prefix + '(A;;KA;;;S-1-5-21-1-2-3-1001)(A;CIID;KR;;;BU)')
        $key = [pscustomobject]@{
            Bytes = $descriptor.GetSecurityDescriptorBinaryForm()
            Writes = 0
            FailRestore = $false
            CorruptRestore = $false
            Exercised = $false
            NormalizeNative = [bool]$NormalizeNative
            DropOwnerGroup = $false
        }
        $key | Add-Member ScriptMethod GetAccessControl {
            param($section)
            if ($section -ne [Security.AccessControl.AccessControlSections]::Access) { throw 'Unexpected security section.' }
            $copy = [Security.AccessControl.RegistrySecurity]::new()
            $copy.SetSecurityDescriptorBinaryForm($this.Bytes, $section)
            $copy | Add-Member NoteProperty NativeBytes ([byte[]]$this.Bytes.Clone())
            $copy | Add-Member ScriptMethod GetSecurityDescriptorBinaryForm { return $this.NativeBytes } -Force
            return $copy
        }
        $key | Add-Member ScriptMethod SetAccessControl {
            param($descriptor)
            $flags = [Reflection.BindingFlags]'Instance,NonPublic'
            $type = [Security.AccessControl.ObjectSecurity]
            $type.GetMethod('ReadLock', $flags).Invoke($descriptor, @())
            try {
                if (-not $type.GetProperty('AccessRulesModified', $flags).GetValue($descriptor, $null)) {
                    throw 'DACL was not marked changed for persistence.'
                }
            } finally { $type.GetMethod('ReadUnlock', $flags).Invoke($descriptor, @()) }
            $this.Writes++
            if ($this.Writes -eq 2 -and $this.FailRestore) { throw 'fixture restore denied' }
            if ($this.Writes -eq 2 -and $this.CorruptRestore) { return }
            $this.Bytes = $descriptor.GetSecurityDescriptorBinaryForm()
            if ($this.NormalizeNative) { $this.Bytes[3] = $this.Bytes[3] -bor 0x04 }
            if ($this.DropOwnerGroup) {
                $raw = [Security.AccessControl.RawSecurityDescriptor]::new($this.Bytes, 0)
                $raw.Owner = $null
                $raw.Group = $null
                $this.Bytes = New-Object byte[] $raw.BinaryLength
                $raw.GetBinaryForm($this.Bytes, 0)
            }
        }
        return $key
    }
    $identity = [Security.Principal.SecurityIdentifier]::new('S-1-5-21-1-2-3-1001')
    foreach ($right in @('SetValue', 'Delete')) {
        $memoryKey = New-MemoryAclKey
        $originalAclBytes = [Convert]::ToBase64String($memoryKey.Bytes)
        Invoke-PrSniperRegistryAclDenial -RestoreKey $memoryKey -Identity $identity -Right $right -Exercise {
            $rules = $memoryKey.GetAccessControl([Security.AccessControl.AccessControlSections]::Access).
                GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier])
            $denies = @($rules | Where-Object { $_.AccessControlType -eq 'Deny' })
            Check ($denies.Count -eq 1 -and $denies[0].RegistryRights.ToString() -ceq $right -and
                $denies[0].IdentityReference -eq $identity) 'Only the intended exact-user right is denied.'
            $memoryKey.Exercised = $true
        }
        Check ($memoryKey.Exercised -and $memoryKey.Writes -eq 2) 'Fixture and restore use the same supplied handle abstraction.'
        Check ([Convert]::ToBase64String($memoryKey.Bytes) -ceq $originalAclBytes) 'Exact protected/inherited DACL bytes restored.'
    }
    $memoryKey = New-MemoryAclKey
    $originalAclBytes = [Convert]::ToBase64String($memoryKey.Bytes)
    Reject { Invoke-PrSniperRegistryAclDenial $memoryKey $identity SetValue { throw 'primary assertion failed' } } 'primary assertion failed'
    Check ([Convert]::ToBase64String($memoryKey.Bytes) -ceq $originalAclBytes) 'Primary failure still restores the original DACL.'
    $memoryKey = New-MemoryAclKey
    $memoryKey.FailRestore = $true
    Reject { Invoke-PrSniperRegistryAclDenial $memoryKey $identity Delete {} } 'fixture restore denied'
    $memoryKey = New-MemoryAclKey
    $memoryKey.CorruptRestore = $true
    Reject { Invoke-PrSniperRegistryAclDenial $memoryKey $identity Delete {} } 'restoration readback differs'
    $memoryKey = New-MemoryAclKey
    $memoryKey.FailRestore = $true
    $script:aclWarnings = @()
    Reject { Invoke-PrSniperRegistryAclDenial $memoryKey $identity SetValue { throw 'primary retained' } -WarningVariable script:aclWarnings } 'primary retained'
    Check (($script:aclWarnings -join ' ') -match 'fixture restore denied') 'Cleanup failure remains visible without replacing the primary failure.'
    foreach ($right in @('SetValue', 'Delete')) {
        $memoryKey = New-MemoryAclKey -Unprotected -NormalizeNative
        $originalBytes = [byte[]]$memoryKey.Bytes.Clone()
        Invoke-PrSniperRegistryAclDenial $memoryKey $identity $right { $memoryKey.Exercised = $true }
        Check ($memoryKey.Exercised -and (Test-PrSniperDaclReadback $originalBytes $memoryKey.Bytes)) `
            'Unprotected inherited DACL admits only the native bookkeeping-bit addition.'
    }
    $expectedAcl = (New-MemoryAclKey -Unprotected).Bytes
    $nativeAcl = [byte[]]$expectedAcl.Clone()
    $nativeAcl[3] = $nativeAcl[3] -bor 0x04
    Check (Test-PrSniperDaclReadback $expectedAcl $nativeAcl) 'Proved SE_DACL_AUTO_INHERITED addition is admitted.'
    Check (-not (Test-PrSniperDaclReadback $nativeAcl $expectedAcl)) 'Unproved removal of the bookkeeping bit is rejected.'
    $represented = [Security.AccessControl.RawSecurityDescriptor]::new(
        'O:S-1-5-21-1-2-3-1001G:S-1-5-21-1-2-3-1002D:(A;;KA;;;S-1-5-21-1-2-3-1001)(A;CIID;KR;;;S-1-5-21-1-2-3-1002)(A;CIID;KR;;;S-1-5-21-1-2)')
    $beforeRepresentation = New-Object byte[] $represented.BinaryLength
    $represented.GetBinaryForm($beforeRepresentation, 0)
    $represented.Owner = $null
    $represented.Group = $null
    $represented.SetFlags($represented.ControlFlags -bor [Security.AccessControl.ControlFlags]::DiscretionaryAclAutoInherited)
    $afterRepresentation = New-Object byte[] $represented.BinaryLength
    $represented.GetBinaryForm($afterRepresentation, 0)
    Check ($beforeRepresentation.Length -eq 184 -and $afterRepresentation.Length -eq 128) 'Reproduce the actual 184-to-128 descriptor representation change with synthetic SIDs.'
    Check (Test-PrSniperDaclReadback $beforeRepresentation $afterRepresentation) 'Identical raw DACL plus permitted bookkeeping survives optional owner/group omission.'
    foreach ($right in @('SetValue', 'Delete')) {
        $memoryKey = New-MemoryAclKey -Unprotected -NormalizeNative
        $memoryKey.Bytes = [byte[]]$beforeRepresentation.Clone()
        $memoryKey.DropOwnerGroup = $true
        Invoke-PrSniperRegistryAclDenial $memoryKey $identity $right { $memoryKey.Exercised = $true }
        Check ($memoryKey.Exercised -and (Test-PrSniperDaclReadback $beforeRepresentation $memoryKey.Bytes)) `
            'Denial and restoration validate the DACL despite a different full descriptor representation.'
    }
    foreach ($change in @('mask','sid','order','ace-type','inheritance','protection','inheritance-request','defaulted','untrusted','server-security','presence','null-acl','empty-acl','revision')) {
        $raw = [Security.AccessControl.RawSecurityDescriptor]::new($expectedAcl, 0)
        switch ($change) {
            mask { $raw.DiscretionaryAcl[0].AccessMask = $raw.DiscretionaryAcl[0].AccessMask -bxor 2 }
            sid { $raw.DiscretionaryAcl[0].SecurityIdentifier = [Security.Principal.SecurityIdentifier]::new('S-1-5-21-1-2-3-1002') }
            order {
                $first = $raw.DiscretionaryAcl[0].Copy()
                $raw.DiscretionaryAcl[0] = $raw.DiscretionaryAcl[1]
                $raw.DiscretionaryAcl[1] = $first
            }
            ace-type {
                $ace = $raw.DiscretionaryAcl[0]
                $raw.DiscretionaryAcl[0] = [Security.AccessControl.CommonAce]::new(
                    $ace.AceFlags, [Security.AccessControl.AceQualifier]::AccessDenied, $ace.AccessMask, $ace.SecurityIdentifier, $false, $null)
            }
            inheritance { $raw.DiscretionaryAcl[1].AceFlags = [Security.AccessControl.AceFlags]::Inherited }
            protection { $raw.SetFlags($raw.ControlFlags -bor [Security.AccessControl.ControlFlags]::DiscretionaryAclProtected) }
            inheritance-request { $raw.SetFlags($raw.ControlFlags -bor [Security.AccessControl.ControlFlags]::DiscretionaryAclAutoInheritRequired) }
            defaulted { $raw.SetFlags($raw.ControlFlags -bor [Security.AccessControl.ControlFlags]::DiscretionaryAclDefaulted) }
            untrusted { $raw.SetFlags($raw.ControlFlags -bor [Security.AccessControl.ControlFlags]::DiscretionaryAclUntrusted) }
            server-security { $raw.SetFlags($raw.ControlFlags -bor [Security.AccessControl.ControlFlags]::ServerSecurity) }
            presence { $raw.SetFlags($raw.ControlFlags -band (-bnot [Security.AccessControl.ControlFlags]::DiscretionaryAclPresent)) }
            null-acl { $raw.DiscretionaryAcl = $null }
            empty-acl { $raw.DiscretionaryAcl = [Security.AccessControl.RawAcl]::new(2, 0) }
        }
        $changedAcl = New-Object byte[] $raw.BinaryLength
        $raw.GetBinaryForm($changedAcl, 0)
        $changedAcl[3] = $changedAcl[3] -bor 0x04
        if ($change -eq 'revision') {
            $changedAcl[[BitConverter]::ToUInt32($changedAcl, 16)] = 4
        }
        Check (-not (Test-PrSniperDaclReadback $expectedAcl $changedAcl)) "Reject real DACL drift despite normalization: $change"
    }
    Check (-not (Test-PrSniperDaclReadback $null $afterRepresentation)) 'Missing descriptor is not valid comparison evidence.'
    Check (-not (Test-PrSniperDaclReadback $expectedAcl ([byte[]]@(1, 2, 3)))) 'Truncated descriptor is rejected.'
    $malformedAcl = [byte[]]$expectedAcl.Clone()
    [Array]::Copy([BitConverter]::GetBytes([uint32]::MaxValue), 0, $malformedAcl, 16, 4)
    Check (-not (Test-PrSniperDaclReadback $expectedAcl $malformedAcl)) 'Malformed DACL offset is rejected, not normalized.'
    $nullDescriptor = [Security.AccessControl.RawSecurityDescriptor]::new(
        [Security.AccessControl.ControlFlags]::DiscretionaryAclPresent, $null, $null, $null, $null)
    $nullDescriptorBytes = New-Object byte[] $nullDescriptor.BinaryLength
    $nullDescriptor.GetBinaryForm($nullDescriptorBytes, 0)
    Check (-not (Test-PrSniperDaclReadback $nullDescriptorBytes $nullDescriptorBytes)) 'Even two null ACLs are not accepted as fixture permission evidence.'
    Check ((Get-PrSniperPayloadEntry @('Path = installer.exe', 'Path = $_41_\new-app.exe', 'Path = $_41_\new-uninstall.exe')) -ceq '$_41_\new-app.exe') `
        'Select the exact application entry, not the installer or uninstaller.'
    Reject { Get-PrSniperPayloadEntry @('Path = new-uninstall.exe') } 'exactly one'
    Reject { Get-PrSniperPayloadEntry @('Path = $_41_\new-app.exe', 'Path = other\new-app.exe') } 'exactly one'
    $profile = Join-Path $fixture 'owned-profile'
    New-Item -ItemType Directory (Join-Path $profile 'state') | Out-Null
    $hostFixture = [pscustomobject]@{ HasExited = $false; MainWindowHandle = [IntPtr]123 }
    $hostFixture | Add-Member ScriptMethod Refresh {}
    Check (-not (Test-PrSniperHostReady $hostFixture $profile)) 'A live helper window alone is not profile readiness.'
    [IO.File]::WriteAllText((Join-Path $profile 'state\notifications.json'), '{"profile_id":"beea9c15-e4ea-4af7-a4e5-687ff7290406","enabled":false}')
    [IO.File]::WriteAllText((Join-Path $profile 'state\diagnostics.jsonl'), '')
    Check (-not (Test-PrSniperHostReady $hostFixture $profile)) 'An empty newly created log is not ready yet.'
    [IO.File]::WriteAllText((Join-Path $profile 'state\diagnostics.jsonl'), "{`"event`":`"session_started`"}`n")
    Check (Test-PrSniperHostReady $hostFixture $profile) 'A nonzero internal HWND must not reject a ready owned host.'
    $hostFixture.HasExited = $true
    Reject { Test-PrSniperHostReady $hostFixture $profile } 'host exited'
    Assert-PrSniperRemovalState -AppExists $false -TransactionExists $false -RegistryValueNames @('ForeignFixture') `
        -ShortcutExists $true -ShortcutTarget 'C:\foreign.exe' -ExpectedApp 'C:\owned\pr-sniper.exe'
    foreach ($name in @('PRSniperInstaller','DisplayVersion','UninstallString','NoModify','DisplayName')) {
        Reject { Assert-PrSniperRemovalState -AppExists $false -TransactionExists $false -RegistryValueNames @($name) `
            -ShortcutExists $false -ExpectedApp 'C:\owned\pr-sniper.exe' } 'registration remains'
    }
    Reject { Assert-PrSniperRemovalState -AppExists $true -TransactionExists $false -RegistryValueNames @() `
        -ShortcutExists $false -ExpectedApp 'C:\owned\pr-sniper.exe' } 'Application removal'
    Reject { Assert-PrSniperRemovalState -AppExists $false -TransactionExists $true -RegistryValueNames @() `
        -ShortcutExists $false -ExpectedApp 'C:\owned\pr-sniper.exe' } 'transaction remains'
    Reject { Assert-PrSniperRemovalState -AppExists $false -TransactionExists $false -RegistryValueNames @() `
        -ShortcutExists $true -ShortcutTarget 'C:\owned\pr-sniper.exe' -ExpectedApp 'C:\owned\pr-sniper.exe' } 'Owned shortcut remains'
    Reject { Assert-PrSniperRemovalState -RegistryKeyExists $true -RegistryValueNames $null `
        -ExpectedApp 'C:\owned\pr-sniper.exe' } 'Empty owned installer key remains'
    Assert-PrSniperRemovalState -RegistryKeyExists $true -RegistrySubKeyCount 1 -RegistryValueNames @() `
        -ExpectedApp 'C:\owned\pr-sniper.exe'
    Assert-PrSniperRemovalState -RegistryKeyExists $true -RegistryValueNames @('') `
        -ExpectedApp 'C:\owned\pr-sniper.exe'
    Check $true 'The unnamed default registry value is foreign nonempty content.'
    $removalReceipt = [pscustomobject]@{ directory = 'C:\owned'; version = '0.1.1'; uninstaller_sha256 = ('a' * 64) }
    $completion = [pscustomobject]@{
        schema = 1; phase = 'native-removal-complete'; installation_receipt_sha256 = ('b' * 64)
        directory = 'C:\owned'; version = '0.1.1'; uninstaller_sha256 = ('a' * 64)
    }
    Assert-PrSniperRemovalReceipt $completion $removalReceipt ('b' * 64)
    Check (-not (Test-PrSniperUninstallerCleanupRequired $completion $removalReceipt ('b' * 64) $false '')) `
        'Valid native completion permits already-absent uninstaller cleanup without deletion.'
    Check (Test-PrSniperUninstallerCleanupRequired $completion $removalReceipt ('b' * 64) $true ('a' * 64)) `
        'An exact remaining uninstaller still requires owned deletion.'
    Reject { Test-PrSniperUninstallerCleanupRequired $null $removalReceipt ('b' * 64) $false '' } 'not evidenced'
    Reject { Test-PrSniperUninstallerCleanupRequired $completion $removalReceipt ('c' * 64) $false '' } 'not evidenced'
    Reject { Test-PrSniperUninstallerCleanupRequired $completion $removalReceipt ('b' * 64) $true ('c' * 64) } 'Uninstaller changed'
    $completion.uninstaller_sha256 = ''
    $removalReceipt.uninstaller_sha256 = ''
    Reject { Test-PrSniperUninstallerCleanupRequired $completion $removalReceipt ('b' * 64) $false '' } 'not evidenced'
    $completion.uninstaller_sha256 = 'a' * 64
    $removalReceipt.uninstaller_sha256 = 'a' * 64
    $chocoFixture = Join-Path $fixture 'chocolatey'
    $activeTools = Join-Path $chocoFixture 'lib\pr-sniper-localtest\tools'
    $backupTools = Join-Path $chocoFixture 'lib-bkp\pr-sniper-localtest\0.1.1-localtest\tools'
    $failedTools = Join-Path $chocoFixture 'lib-bad\pr-sniper-localtest\0.1.1-localtest\tools'
    $appFixture = Join-Path $fixture 'application'
    foreach ($path in @($activeTools, $backupTools, $failedTools, $appFixture)) {
        New-Item -ItemType Directory $path | Out-Null
    }
    [IO.File]::WriteAllText((Join-Path $appFixture 'uninstall.exe'), 'inert never-executed fixture')
    $rollbackReceipt = [pscustomobject]@{
        installation_id = '727d86c7-d485-4190-9140-7168128d9a39'
        directory = $appFixture; version = '0.1.1'
        uninstaller_sha256 = (Get-FileHash (Join-Path $appFixture 'uninstall.exe')).Hash
    }
    $rollbackReceipt | ConvertTo-Json | Set-Content (Join-Path $activeTools 'installation.json')
    $rollbackReceiptHash = (Get-FileHash (Join-Path $activeTools 'installation.json')).Hash
    Initialize-PrSniperRemovalState $activeTools $rollbackReceipt $rollbackReceiptHash
    foreach ($name in @('installation.json','native-removal.json','native-removal.pending.json')) {
        Copy-Item (Join-Path $activeTools $name) (Join-Path $backupTools $name)
    }
    Complete-PrSniperNativeRemoval $activeTools $rollbackReceipt $rollbackReceiptHash
    Check ((Get-PrSniperRemovalState $activeTools $rollbackReceipt $rollbackReceiptHash).completed) `
        'Native success commits completion before the package manager handles failure.'
    foreach ($name in @('installation.json','native-removal.json')) {
        Copy-Item (Join-Path $activeTools $name) (Join-Path $failedTools $name)
    }
    # Pinned Chocolatey failure handling moves the failed tree aside, then
    # restores its before-modify snapshot, including the deleted pending marker.
    Copy-Item (Join-Path $backupTools 'native-removal.pending.json') (Join-Path $activeTools 'native-removal.pending.json')
    $restored = Get-PrSniperRemovalState $activeTools $rollbackReceipt $rollbackReceiptHash
    Check (-not $restored.completed) 'Replayed package rollback proves the current pending-delete protocol loses active completion.'
    $observation = Get-PrSniperRemovalRetryObservation $activeTools $appFixture $chocoFixture `
        0.1.1 $rollbackReceipt.uninstaller_sha256 $rollbackReceiptHash $restored.completed 1
    Check (-not $observation.observed.app.exists -and
        $observation.observed.uninstaller.sha256 -ieq $rollbackReceipt.uninstaller_sha256) 'Post-outer capture distinguishes app absence from changed uninstaller bytes.'
    Check ($observation.observed.active['native-removal.pending.json'].exists -and
        -not $observation.observed.failed_copy['native-removal.pending.json'].exists -and
        $observation.observed.backup_copy['native-removal.pending.json'].exists) 'Capture distinguishes active/restored and exact failed/backup copies.'
    Check ($observation.observed.active['installation.json'].sha256 -ceq
        $observation.observed.failed_copy['installation.json'].sha256) 'Capture preserves exact receipt correspondence without adopting it.'
    Reject { Get-PrSniperRemovalRetryObservation $activeTools $appFixture $chocoFixture '../other' '' '' $false 1 } 'Invalid version'
    $durablePath = Get-PrSniperDurableRemovalPath $activeTools $chocoFixture $rollbackReceipt $rollbackReceiptHash
    Complete-PrSniperNativeRemoval $activeTools $rollbackReceipt $rollbackReceiptHash $durablePath
    $durableHash = (Get-FileHash $durablePath).Hash
    foreach ($attempt in 1..2) {
        # Each outer failure replaces the versioned failed copy and restores
        # the original pending marker. The sibling completion must survive both.
        foreach ($name in @('installation.json','native-removal.json','native-removal.pending.json')) {
            Copy-Item (Join-Path $backupTools $name) (Join-Path $failedTools $name) -Force
            Copy-Item (Join-Path $backupTools $name) (Join-Path $activeTools $name) -Force
        }
        $recovered = Get-PrSniperRemovalState $activeTools $rollbackReceipt $rollbackReceiptHash $durablePath
        Check ($recovered.completed -and (Get-FileHash $durablePath).Hash -ceq $durableHash) `
            'Exact completion survives repeated outer rollback, including before a mutex can be acquired.'
        Check (Test-Path (Join-Path $activeTools 'native-removal.pending.json')) `
            'Reading completion does not mutate a restored marker outside the lifecycle lock.'
    }
    Check ($durablePath -ieq (Join-Path $chocoFixture "lib-bad\pr-sniper-localtest\native-removal-$($rollbackReceiptHash.ToLowerInvariant()).json")) `
        'Durable completion is outside the replaceable versioned failed copy, inside package-owned outer cleanup.'
    Reject { Get-PrSniperDurableRemovalPath $activeTools (Join-Path $chocoFixture 'foreign-root') $rollbackReceipt $rollbackReceiptHash } 'exact Chocolatey'
    Reject { Get-PrSniperDurableRemovalPath $activeTools '' $rollbackReceipt $rollbackReceiptHash } 'exact Chocolatey'
    Reject { Get-PrSniperDurableRemovalPath $activeTools $chocoFixture $removalReceipt $rollbackReceiptHash } 'uniquely identified'
    Reject { Get-PrSniperDurableRemovalPath $activeTools $chocoFixture $rollbackReceipt '../foreign' } 'uniquely identified'
    $durableBytes = [IO.File]::ReadAllBytes($durablePath)
    foreach ($field in @('schema','phase','directory','version','uninstaller_sha256','installation_receipt_sha256')) {
        $changed = [Text.Encoding]::UTF8.GetString($durableBytes) | ConvertFrom-Json
        $changed.$field = 'foreign-or-invalid'
        $changed | ConvertTo-Json | Set-Content $durablePath
        Reject { Get-PrSniperRemovalState $activeTools $rollbackReceipt $rollbackReceiptHash $durablePath } 'not evidenced'
    }
    [IO.File]::WriteAllText($durablePath, '{')
    Reject { Get-PrSniperRemovalState $activeTools $rollbackReceipt $rollbackReceiptHash $durablePath } '.'
    [IO.File]::WriteAllBytes($durablePath, $durableBytes)
    $lockedReceipt = [IO.File]::Open($durablePath, 'Open', 'Read', 'None')
    try {
        Reject { Get-PrSniperRemovalState $activeTools $rollbackReceipt $rollbackReceiptHash $durablePath } '.'
    } finally { $lockedReceipt.Dispose() }
    $pendingRollbackPath = Join-Path $activeTools 'native-removal.pending.json'
    $pendingRollbackBytes = [IO.File]::ReadAllBytes($pendingRollbackPath)
    [IO.File]::WriteAllText($pendingRollbackPath, '{}')
    Reject { Get-PrSniperRemovalState $activeTools $rollbackReceipt $rollbackReceiptHash $durablePath } 'Invalid pending'
    [IO.File]::WriteAllBytes($pendingRollbackPath, $pendingRollbackBytes)
    # A distinct same-version installation must not inherit the old completion,
    # even when its directory and uninstaller bytes are identical.
    $reinstalled = $rollbackReceipt | ConvertTo-Json | ConvertFrom-Json
    $reinstalled.installation_id = '5f478293-9983-4990-afef-c6e1b2d1bf66'
    $reinstallBytes = [Text.Encoding]::UTF8.GetBytes(($reinstalled | ConvertTo-Json))
    $sha = [Security.Cryptography.SHA256]::Create()
    try { $reinstallHash = ([BitConverter]::ToString($sha.ComputeHash($reinstallBytes))).Replace('-', '') }
    finally { $sha.Dispose() }
    $reinstallPath = Get-PrSniperDurableRemovalPath $activeTools $chocoFixture $reinstalled $reinstallHash
    Check ($reinstallPath -ine $durablePath -and -not (Test-Path $reinstallPath)) 'Same-version reinstall has a distinct completion identity.'
    Reject { Read-PrSniperDurableRemoval $durablePath $reinstalled $reinstallHash } 'not evidenced'
    Remove-Item (Join-Path $appFixture 'uninstall.exe')
    Check (-not (Test-PrSniperUninstallerCleanupRequired $recovered.receipt $rollbackReceipt $rollbackReceiptHash $false '')) `
        'Durable exact completion also permits retry with the original uninstaller already gone.'
    # After the caller's locked live-state checks, reconcile the active marker.
    Complete-PrSniperNativeRemoval $activeTools $rollbackReceipt $rollbackReceiptHash $durablePath
    Check ((Get-PrSniperRemovalState $activeTools $rollbackReceipt $rollbackReceiptHash).completed -and
        (Get-FileHash $durablePath).Hash -ceq $durableHash) 'Reconciliation preserves the durable receipt and restores active completion.'
    # Model successful Chocolatey cleanup of its exact package failure tree,
    # then installed-snapshot removal. No production script deletes the durable receipt.
    Remove-Item -LiteralPath $durablePath
    foreach ($name in @('installation.json','native-removal.json')) {
        Check ((Get-FileHash (Join-Path $activeTools $name)).Hash -ceq (Get-FileHash (Join-Path $backupTools $name)).Hash) `
            'Active immutable records retain their installed-snapshot checksums after recovery.'
        Remove-Item -LiteralPath (Join-Path $activeTools $name)
    }
    Check (-not (Get-ChildItem $activeTools -File) -and -not (Test-Path $durablePath)) 'Successful cleanup leaves neither active state nor durable completion.'
    $directoryReceipt = Join-Path $fixture 'not-a-receipt'
    New-Item -ItemType Directory $directoryReceipt | Out-Null
    Reject { Read-PrSniperDurableRemoval $directoryReceipt $rollbackReceipt $rollbackReceiptHash } 'ordinary receipt file'
    Remove-Item -LiteralPath $directoryReceipt
    $stateTools = Join-Path $fixture 'tracked-state'
    New-Item -ItemType Directory $stateTools | Out-Null
    Initialize-PrSniperRemovalState $stateTools $removalReceipt ('b' * 64)
    $statePath = Join-Path $stateTools 'native-removal.json'
    $pendingPath = Join-Path $stateTools 'native-removal.pending.json'
    $trackedHashes = @{}
    foreach ($file in Get-ChildItem $stateTools -File) { $trackedHashes[$file.FullName] = (Get-FileHash $file.FullName).Hash }
    Check ($trackedHashes.Count -eq 2 -and -not (Get-PrSniperRemovalState $stateTools $removalReceipt ('b' * 64)).completed) `
        'Both immutable records exist at install tracking time and initially mean not completed.'
    Reject { Initialize-PrSniperRemovalState $stateTools $removalReceipt ('b' * 64) } 'nothing was overwritten'
    Reject { Get-PrSniperRemovalState $stateTools $removalReceipt ('c' * 64) } 'not evidenced'
    Complete-PrSniperNativeRemoval $stateTools $removalReceipt ('b' * 64)
    $committed = Get-PrSniperRemovalState $stateTools $removalReceipt ('b' * 64)
    Check ($committed.completed -and -not (Test-Path $pendingPath)) 'Only the pending marker is removed at native completion.'
    Check ((Get-FileHash $statePath).Hash -ceq $trackedHashes[$statePath]) 'Completion evidence retains its original tracked checksum.'
    Check (-not (Test-PrSniperUninstallerCleanupRequired $committed.receipt $removalReceipt ('b' * 64) $false '')) `
        'Completed evidence survives an outer-cleanup failure with the uninstaller already absent.'
    Reject { Complete-PrSniperNativeRemoval $stateTools $removalReceipt ('b' * 64) } 'must not be repeated'
    # Model Chocolatey 2.7.4: only installed-snapshot paths with equal checksums
    # are removed. This catches both newly created and mutated receipt residues.
    foreach ($file in Get-ChildItem $stateTools -File) {
        if ($trackedHashes.ContainsKey($file.FullName) -and (Get-FileHash $file.FullName).Hash -ceq $trackedHashes[$file.FullName]) {
            Remove-Item -LiteralPath $file.FullName
        }
    }
    Check (-not (Get-ChildItem $stateTools -File)) 'Tracked outer cleanup leaves no completion state.'
    Initialize-PrSniperRemovalState $stateTools $removalReceipt ('b' * 64)
    Check (-not (Get-PrSniperRemovalState $stateTools $removalReceipt ('b' * 64)).completed) 'Same-package reinstall starts noncompleted, not poisoned by an old receipt.'
    $pendingBytes = [IO.File]::ReadAllBytes($pendingPath)
    [IO.File]::WriteAllText($pendingPath, '{}')
    Reject { Get-PrSniperRemovalState $stateTools $removalReceipt ('b' * 64) } 'Invalid pending'
    [IO.File]::WriteAllBytes($pendingPath, $pendingBytes)
    [IO.File]::WriteAllText($statePath, '{"schema":1,"phase":"native-removal-complete"}')
    Reject { Get-PrSniperRemovalState $stateTools $removalReceipt ('b' * 64) } 'Legacy/unrecognized'
    $missingUninstaller = Get-PrSniperUninstallerFileState (Join-Path $fixture 'absent-uninstaller.exe')
    Check (-not $missingUninstaller.exists -and $missingUninstaller.sha256 -ceq '') 'Only actual file absence produces absent evidence.'
    Reject { Get-PrSniperUninstallerFileState $fixture } 'not a regular file'
    Reject { Assert-PrSniperRemovalReceipt $null $removalReceipt ('b' * 64) } 'not evidenced'
    foreach ($field in @('schema','phase','directory','version','uninstaller_sha256','installation_receipt_sha256')) {
        $old = $completion.$field
        $completion.$field = 'wrong-or-pending'
        Reject { Assert-PrSniperRemovalReceipt $completion $removalReceipt ('b' * 64) } 'not evidenced'
        $completion.$field = $old
    }
    $files = @(Get-ChildItem (Join-Path $repository 'scripts\windows-*.ps1')) +
        @(Get-ChildItem (Join-Path $repository 'packaging\chocolatey\*.ps1'))
    foreach ($file in $files) {
        $parseErrors = $null
        $tokens = $null
        [Management.Automation.Language.Parser]::ParseFile($file.FullName, [ref]$tokens, [ref]$parseErrors) | Out-Null
        Check ($parseErrors.Count -eq 0) "PowerShell parse errors: $($file.Name): $parseErrors"
    }
    Reject { & (Join-Path $repository 'scripts\windows-installer-acceptance.ps1') -Candidate $fixture -Upgrade $fixture } 'forbidden outside'
    Reject { & (Join-Path $repository 'scripts\windows-upgrade-fixture.ps1') } 'only on a hosted'
    Reject { & (Join-Path $repository 'scripts\windows-installer-faults.ps1') -Phase Install -Installer 'not-executed.exe' } 'require the owned hosted'
    Reject { & (Join-Path $repository 'scripts\windows-removal-retry.ps1') } 'requires owned hosted'
    $executable = Join-Path $fixture 'never-execute.exe'
    Add-Type -OutputAssembly $executable -OutputType WindowsApplication -TypeDefinition @'
using System.Reflection;
[assembly: AssemblyProduct("PR Sniper")]
[assembly: AssemblyInformationalVersion("0.1.1")]
[assembly: AssemblyFileVersion("0.1.1.0")]
public class PackagingFixture { public static void Main() {} }
'@
    $hash = (Get-FileHash $executable).Hash
    $presentUninstaller = Get-PrSniperUninstallerFileState $executable
    Check ($presentUninstaller.exists -and $presentUninstaller.sha256 -ceq $hash) 'Present-file observation includes its actual SHA256.'
    $arguments = @{
        Mode = 'LocalTest'; Installer = $executable; Version = '0.1.1'
        Sha256 = $hash; Commit = ('a' * 40); Destination = (Join-Path $fixture 'package')
    }
    $generator = Join-Path $repository 'scripts\windows-chocolatey.ps1'
    & $generator @arguments | Out-Null
    $nuspec = [xml](Get-Content (Join-Path $arguments.Destination 'pr-sniper-localtest.nuspec') -Raw)
    Check ($nuspec.package.metadata.id -ceq 'pr-sniper-localtest') 'Test package must not use the public ID.'
    Check ($nuspec.package.metadata.version -ceq '0.1.1-localtest') 'Test package must be a prerelease.'
    Check ($nuspec.package.metadata.authors -ceq 'Dylan McCurry') 'Preserve actual authorship.'
    Check (-not $nuspec.package.metadata.licenseUrl) 'Do not invent a project license.'
    Check ((Get-FileHash (Join-Path $arguments.Destination 'tools\installer.exe')).Hash -ceq $hash) 'Pin exact local bytes.'
    $installScript = Join-Path $arguments.Destination 'tools\chocolateyinstall.ps1'
    Reject { & $installScript } 'explicit consent for this exact installer'
    $env:PR_SNIPER_UNSIGNED_LOCAL_TEST_SHA256 = '0' * 64
    Reject { & $installScript } 'explicit consent for this exact installer'
    function Start-ChocolateyProcessAsAdmin { throw 'Fixture boundary reached; no executable started.' }
    $env:PR_SNIPER_UNSIGNED_LOCAL_TEST_SHA256 = $hash
    Reject { & $installScript } 'Fixture boundary reached'
    $metadataPath = Join-Path $arguments.Destination 'tools\installer.json'
    $originalMetadata = [IO.File]::ReadAllBytes($metadataPath)
    $publicMetadata = Get-Content $metadataPath -Raw | ConvertFrom-Json
    $publicMetadata.mode = 'Public'
    $publicMetadata.url = 'https://github.com/jdylanmc/pr-sniper/releases/download/v0.1.1/pr-sniper-0.1.1-x64-setup.exe'
    $publicMetadata.signer_thumbprint = 'A' * 40
    $publicMetadata.signer_subject = 'CN=Fixture'
    $publicMetadata | ConvertTo-Json | Set-Content $metadataPath -Encoding utf8
    function Get-ChocolateyWebFile { }
    Reject { & $installScript } 'Windows-trusted embedded Authenticode'
    [IO.File]::WriteAllBytes($metadataPath, $originalMetadata)
    $env:PR_SNIPER_UNSIGNED_LOCAL_TEST_SHA256 = ''
    $arguments.Destination = Join-Path $fixture 'duplicate'
    & $generator @arguments | Out-Null
    foreach ($file in Get-ChildItem (Join-Path $fixture 'package') -File -Recurse) {
        $relative = $file.FullName.Substring((Join-Path $fixture 'package').Length + 1)
        Check ((Get-FileHash $file.FullName).Hash -ceq (Get-FileHash (Join-Path $arguments.Destination $relative)).Hash) "Nondeterministic source: $relative"
    }
    $arguments.Destination = Join-Path $fixture 'rejected'
    $arguments.Sha256 = '0' * 64
    Reject { & $generator @arguments } 'checksum mismatch'
    Check (-not (Test-Path $arguments.Destination)) 'Rejected generation must not create package output.'
    $arguments.Sha256 = $hash
    $arguments.Version = '0.1.2'
    Reject { & $generator @arguments } 'identity/version mismatch'
    $arguments.Version = '0.1.1'
    $arguments.Mode = 'Public'
    Reject { & $generator @arguments } 'Public packaging requires'
    $arguments.Url = 'https://github.com/jdylanmc/pr-sniper/releases/download/v0.1.1/pr-sniper-0.1.1-x64-setup.exe'
    $arguments.ExpectedThumbprint = 'A' * 40
    $arguments.ExpectedSubject = 'CN=Test'
    $arguments.LicenseUrl = 'https://example.invalid/fixture-only'
    Reject { & $generator @arguments } 'Windows-trusted embedded Authenticode'

    . (Join-Path $repository 'scripts\windows-signature.ps1')
    $script:signature = [pscustomobject]@{
        Status = 'Valid'; SignatureType = 'Authenticode'
        TimeStamperCertificate = [pscustomobject]@{ Subject = 'CN=Timestamp' }
        SignerCertificate = [pscustomobject]@{
            Thumbprint = ('A' * 40); Subject = 'CN=Publisher'; Issuer = 'CN=Issuer'
            EnhancedKeyUsageList = @([pscustomobject]@{ ObjectId = '1.3.6.1.5.5.7.3.3' })
        }
    }
    function Get-AuthenticodeSignature { param($LiteralPath, $ErrorAction) return $script:signature }
    function Get-Command { param($Name, $ErrorAction) return @{ Source = 'Invoke-FixtureSignTool' } }
    $script:verifierCode = 0
    function Invoke-FixtureSignTool { $global:LASTEXITCODE = $script:verifierCode }
    foreach ($status in @('NotSigned', 'HashMismatch', 'NotTrusted', 'UnknownError')) {
        $signature.Status = $status
        Reject { Assert-TrustedWindowsSignature $executable ('A' * 40) 'CN=Publisher' } 'Windows-trusted'
    }
    $signature.Status = 'Valid'
    $timestamp = $signature.TimeStamperCertificate
    $signature.TimeStamperCertificate = $null
    Reject { Assert-TrustedWindowsSignature $executable ('A' * 40) 'CN=Publisher' } 'timestamp'
    $signature.TimeStamperCertificate = $timestamp
    Reject { Assert-TrustedWindowsSignature $executable ('B' * 40) 'CN=Publisher' } 'publisher differs'
    Reject { Assert-TrustedWindowsSignature $executable ('A' * 40) 'CN=Other' } 'publisher differs'
    $signature.SignerCertificate.Issuer = 'CN=Publisher'
    Reject { Assert-TrustedWindowsSignature $executable ('A' * 40) 'CN=Publisher' } 'publisher differs'
    $signature.SignerCertificate.Issuer = 'CN=Issuer'
    $signature.SignerCertificate.EnhancedKeyUsageList = @()
    Reject { Assert-TrustedWindowsSignature $executable ('A' * 40) 'CN=Publisher' } 'not authorized for code signing'
    $signature.SignerCertificate.EnhancedKeyUsageList = @([pscustomobject]@{ ObjectId = '1.3.6.1.5.5.7.3.3' })
    Assert-TrustedWindowsSignature $executable ('A' * 40) 'CN=Publisher' -RequireSignTool
    $script:verifierCode = 1
    Reject { Assert-TrustedWindowsSignature $executable ('A' * 40) 'CN=Publisher' -RequireSignTool } 'SignTool rejected'
    "Windows packaging: $count assertions passed. No installer or app executed; no registry/credential writes."
} finally {
    $env:TEMP = $originalTemp
    $env:TMP = $originalTmp
    $env:GITHUB_ACTIONS = $originalActions
    $env:PR_SNIPER_UNSIGNED_LOCAL_TEST_SHA256 = $originalLocalConsent
    $env:PR_SNIPER_INSTALLER_DIAGNOSTICS = $originalDiagnostics
    Remove-Item -LiteralPath $fixture -Recurse -Force
}

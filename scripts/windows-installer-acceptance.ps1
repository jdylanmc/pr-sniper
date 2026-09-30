param(
    [Parameter(Mandatory)][string] $Candidate,
    [Parameter(Mandatory)][string] $Upgrade
)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or
    $env:RUNNER_OS -ne 'Windows' -or $env:GITHUB_REPOSITORY -ne 'jdylanmc/pr-sniper') {
    throw 'Installation acceptance is forbidden outside this repository on a fresh GitHub-hosted Windows VM.'
}
$repository = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'windows-host-readiness.ps1')
$directory = Join-Path $env:LOCALAPPDATA 'PR Sniper'
$data = Join-Path $env:LOCALAPPDATA 'com.jdylanmc.pr-sniper'
$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'PR Sniper.lnk'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\PR Sniper'
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
if ((Get-Process -Name 'pr-sniper' -ErrorAction SilentlyContinue) -or
    (Test-Path $directory) -or (Test-Path $data) -or (Test-Path $shortcut) -or
    (Test-Path $uninstallKey) -or
    (Test-Path 'HKCU:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\PR Sniper') -or
    (Test-Path 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\PR Sniper') -or
    (Test-Path 'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\PR Sniper') -or
    (Test-Path (Join-Path ([Environment]::GetFolderPath('DesktopDirectory')) 'PR Sniper.lnk')) -or
    ($env:ChocolateyInstall -and ((Test-Path "$env:ChocolateyInstall\lib\pr-sniper") -or
        (Test-Path "$env:ChocolateyInstall\lib\pr-sniper-localtest"))) -or
    (Get-ChildItem ([Environment]::GetFolderPath('Programs')) -Filter 'PR Sniper notifications*.lnk') -or
    (Get-ItemProperty $runKey -Name 'com.jdylanmc.pr-sniper','PR Sniper' -ErrorAction SilentlyContinue)) {
    throw 'Runner contains an existing application/profile/registration. Refusing takeover.'
}
$base = Get-Content (Join-Path $Candidate 'installer.json') -Raw | ConvertFrom-Json
$next = Get-Content (Join-Path $Upgrade 'fixture.json') -Raw | ConvertFrom-Json
if ($base.commit -cne $env:GITHUB_SHA -or $next.commit -cne $base.commit -or
    $base.distribution -cne 'unsigned-ci-candidate-not-a-public-release' -or
    $next.distribution -cne 'disposable-upgrade-fixture-never-release' -or
    $next.base_version -cne $base.version -or [version]$next.version -le [version]$base.version) {
    throw 'Candidate/upgrade provenance mismatch.'
}
$workspace = Join-Path $repository 'src-tauri\target\windows-acceptance'
New-Item -ItemType Directory $workspace -ErrorAction Stop | Out-Null
$feed = New-Item -ItemType Directory (Join-Path $workspace 'feed')
$diagnostics = New-Item -ItemType Directory (Join-Path $workspace 'installer-diagnostics')
$env:PR_SNIPER_INSTALLER_DIAGNOSTICS = $diagnostics.FullName
[ordered]@{
    commit = $base.commit
    version = $base.version
    installer_sha256 = $base.sha256
    upgrade_version = $next.version
    upgrade_sha256 = $next.sha256
    purpose = 'unsigned-hosted-installer-diagnostics-not-acceptance'
} | ConvertTo-Json | Set-Content (Join-Path $diagnostics.FullName 'context.json') -Encoding utf8
$env:PR_SNIPER_PACKAGING_ACCEPTANCE = '1'
$env:PR_SNIPER_DATA_DIR = Join-Path $workspace 'profile'
$env:PR_SNIPER_KEYCHAIN_SERVICE = 'com.jdylanmc.pr-sniper.tests.' + [guid]::NewGuid()
$credential = $env:PR_SNIPER_KEYCHAIN_SERVICE + '.packaging-preservation'
$sentinel = [guid]::NewGuid().ToString()
$ownedHost = $null
$credentialCreated = $false
$failure = $null
function Invoke-Choco([string[]] $Arguments) {
    & choco @Arguments --yes --no-progress --limit-output --execution-timeout=180
    if ($LASTEXITCODE -ne 0) { throw "Chocolatey $($Arguments[0]) failed with exit code $LASTEXITCODE." }
}
function Assert-Preserved {
    if ((Get-Content (Join-Path $data 'packaging-preservation.txt') -Raw).Trim() -cne $sentinel) {
        throw 'Application data was not preserved.'
    }
    $record = & cmdkey.exe "/list:$credential"
    if ($LASTEXITCODE -ne 0 -or -not ($record -match [regex]::Escape($credential))) {
        throw 'Exact synthetic credential was not preserved.'
    }
    if ((Get-ItemPropertyValue $runKey 'com.jdylanmc.pr-sniper') -cne $sentinel -or
        (Get-ItemPropertyValue $runKey 'PR Sniper') -cne $sentinel) {
        throw 'Installer changed an unowned startup value.'
    }
}
function Assert-Installed($Metadata, [string] $Installer) {
    $app = Join-Path $directory 'pr-sniper.exe'
    $observedHash = (Get-FileHash $app -Algorithm SHA256).Hash.ToLowerInvariant()
    $observedVersion = [Diagnostics.FileVersionInfo]::GetVersionInfo($app).ProductVersion
    $registeredVersion = Get-ItemPropertyValue $uninstallKey 'DisplayVersion'
    $mismatch = $observedHash -ine $Metadata.application_sha256 -or
        $observedVersion -cne $Metadata.version -or $registeredVersion -cne $Metadata.version
    try {
        [ordered]@{
            commit = $Metadata.commit
            expected_sha256 = $Metadata.application_sha256
            observed_sha256 = $observedHash
            expected_version = $Metadata.version
            observed_pe_version = $observedVersion
            observed_registration_version = $registeredVersion
        } | ConvertTo-Json | Set-Content (Join-Path $diagnostics.FullName ("installed-" + [guid]::NewGuid() + '.json')) -Encoding utf8
    } catch {
        if (-not $mismatch) { throw }
        Write-Warning 'Installed-state diagnostics could not be saved; preserving the candidate mismatch.'
    }
    if ($mismatch) {
        throw "Installed candidate mismatch: expected SHA256=$($Metadata.application_sha256), observed SHA256=$observedHash; expected version=$($Metadata.version), PE version=$observedVersion, registered version=$registeredVersion."
    }
    $quotedValues = [ordered]@{
        DisplayIcon = "`"$app`""
        UninstallString = "`"$directory\uninstall.exe`""
        QuietUninstallString = "`"$directory\uninstall.exe`" /S"
    }
    $registration = Get-Item -LiteralPath $uninstallKey
    try {
        foreach ($name in $quotedValues.Keys) {
            if ($registration.GetValueKind($name) -ne [Microsoft.Win32.RegistryValueKind]::String -or
                $registration.GetValue($name) -cne $quotedValues[$name]) {
                throw "Installed REG_SZ type or quoted value differs from the expected registration: $name."
            }
        }
    } finally { $registration.Dispose() }
    if (Get-Process -Name 'pr-sniper' -ErrorAction SilentlyContinue) { throw 'Installer silently launched the app.' }
    $env:PR_SNIPER_DATA_DIR = Join-Path $workspace ('profile-' + [guid]::NewGuid())
    $script:ownedHost = Start-Process $app -WorkingDirectory $workspace -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    while (-not (Test-PrSniperHostReady $script:ownedHost $env:PR_SNIPER_DATA_DIR)) {
        if ([DateTime]::UtcNow -ge $deadline) { throw 'Installed host did not initialize its fresh owned profile.' }
        Start-Sleep -Milliseconds 100
    }
    $blocked = Start-Process $Installer -ArgumentList '/S' -PassThru
    if (-not $blocked.WaitForExit(30000)) {
        Stop-Process -Id $blocked.Id
        throw 'Installer did not promptly reject a running application.'
    }
    $script:ownedHost.Refresh()
    if ($blocked.ExitCode -eq 0 -or $script:ownedHost.HasExited) {
        throw 'Installer failed to preserve and reject the running owned host.'
    }
    # Only the exact process launched above; not a process-name kill or GUI proof.
    Stop-Process -Id $script:ownedHost.Id -ErrorAction Stop
    $script:ownedHost.WaitForExit()
    $script:ownedHost = $null
}
try {
    & choco --version
    if ($LASTEXITCODE -ne 0) { throw 'Hosted runner Chocolatey CLI is required.' }
    $packages = @(
        @{ metadata = $base; installer = (Join-Path $Candidate $base.filename) },
        @{ metadata = $next; installer = (Join-Path $Upgrade 'upgrade-test-only.exe') }
    )
    if ($next.webview_fixture -notmatch '^Software\\PRSniperInstallerTests\\[a-fA-F0-9-]{36}$') {
        throw 'Missing owned synthetic WebView2 lookup fixture.'
    }
    $webviewFixturePath = 'HKCU:\' + $next.webview_fixture
    if (Test-Path $webviewFixturePath) { throw 'WebView2 fixture key already exists.' }
    New-Item $webviewFixturePath -Force | Out-Null
    New-ItemProperty $webviewFixturePath 'pv' -Value '120.0.0.0' -PropertyType String | Out-Null
    foreach ($package in $packages) {
        $metadata = $package.metadata
        $source = Join-Path $workspace $metadata.version
        & (Join-Path $PSScriptRoot 'windows-chocolatey.ps1') -Mode LocalTest -Installer $package.installer `
            -Version $metadata.version -Sha256 $metadata.sha256 -Commit $metadata.commit -Destination $source
        Invoke-Choco @('pack', (Join-Path $source 'pr-sniper-localtest.nuspec'), "--output-directory=$($feed.FullName)")
    }
    New-Item -ItemType Directory $data -ErrorAction Stop | Out-Null
    $sentinel | Set-Content (Join-Path $data 'packaging-preservation.txt')
    & cmdkey.exe "/generic:$credential" '/user:packaging-fixture' '/pass:non-secret-fixture' | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Cannot create exact synthetic credential.' }
    $credentialCreated = $true
    New-Item $runKey -Force | Out-Null
    New-ItemProperty $runKey 'com.jdylanmc.pr-sniper' -Value $sentinel -PropertyType String | Out-Null
    New-ItemProperty $runKey 'PR Sniper' -Value $sentinel -PropertyType String | Out-Null
    Invoke-Choco @('install', 'pr-sniper-localtest', "--version=$($base.version)-localtest", '--pre', "--source=$($feed.FullName)")
    Assert-Installed $base (Join-Path $Candidate $base.filename)
    Assert-Preserved
    & (Join-Path $PSScriptRoot 'windows-installer-faults.ps1') -Phase Install `
        -Installer (Join-Path $Upgrade 'upgrade-test-only.exe') -PreviousInstaller (Join-Path $Candidate $base.filename)
    Invoke-Choco @('upgrade', 'pr-sniper-localtest', "--version=$($next.version)-localtest", '--pre', "--source=$($feed.FullName)")
    Assert-Installed $next (Join-Path $Upgrade 'upgrade-test-only.exe')
    Assert-Preserved
    & (Join-Path $PSScriptRoot 'windows-installer-faults.ps1') -Phase Uninstall
    # After clearing Delete-only denial, prove empty-key removal and reinstall,
    # separately from the intentionally nonempty foreign-container case below.
    Invoke-Choco @('uninstall', 'pr-sniper-localtest')
    if (Test-Path $uninstallKey) { throw 'Clean uninstall retained its empty installer key.' }
    Invoke-Choco @('install', 'pr-sniper-localtest', "--version=$($next.version)-localtest", '--pre', "--source=$($feed.FullName)")
    Assert-Installed $next (Join-Path $Upgrade 'upgrade-test-only.exe')
    Assert-Preserved
    # Prove the uninstaller preserves foreign contents, not merely a clean dir.
    $sentinel | Set-Content (Join-Path $directory 'foreign-file.txt')
    New-ItemProperty $uninstallKey 'ForeignFixture' -Value $sentinel | Out-Null
    New-Item "$uninstallKey\ForeignFixtureChild" | Out-Null
    New-ItemProperty "$uninstallKey\ForeignFixtureChild" 'marker' -Value $sentinel | Out-Null
    $shell = New-Object -ComObject WScript.Shell
    $link = $shell.CreateShortcut($shortcut)
    if ($link.TargetPath -ine (Join-Path $directory 'pr-sniper.exe')) { throw 'Installed shortcut target mismatch.' }
    $link.TargetPath = $env:ComSpec
    $link.Save()
    & (Join-Path $PSScriptRoot 'windows-removal-retry.ps1')
    Assert-Preserved
    if ((Test-Path (Join-Path $directory 'pr-sniper.exe')) -or
        (Test-Path (Join-Path $directory 'uninstall.exe')) -or
        (Get-ItemProperty $uninstallKey -Name DisplayName -ErrorAction SilentlyContinue) -or
        (Get-ItemPropertyValue $uninstallKey 'ForeignFixture') -cne $sentinel -or
        (Get-ItemPropertyValue "$uninstallKey\ForeignFixtureChild" 'marker') -cne $sentinel -or
        (Get-Content (Join-Path $directory 'foreign-file.txt') -Raw).Trim() -cne $sentinel -or
        $shell.CreateShortcut($shortcut).TargetPath -ine $env:ComSpec) {
        throw 'Owned removal or foreign-content preservation failed.'
    }
    [ordered]@{
        commit = $base.commit
        installer_sha256 = $base.sha256
        test_upgrade_version = $next.version
        test_upgrade_sha256 = $next.sha256
        result = 'local-feed-install-upgrade-uninstall-passed'
        gui_acceptance = 'not-proven-by-live-host-and-owned-profile-readiness'
        public_distribution = $false
    } | ConvertTo-Json | Set-Content (Join-Path $workspace 'acceptance.json')
} catch {
    $failure = $_
    try {
        [ordered]@{ status = 'failed'; commit = $base.commit; exception_type = $_.Exception.GetType().FullName } |
            ConvertTo-Json | Set-Content (Join-Path $diagnostics.FullName 'result.json') -Encoding utf8
        foreach ($trace in Get-ChildItem -LiteralPath $diagnostics.FullName -Filter 'nsis-*.txt' -File) {
            Write-Host "Native installer trace: $($trace.Name)"
            Get-Content -LiteralPath $trace.FullName -Encoding Unicode | ForEach-Object { Write-Host $_ }
        }
    } catch { Write-Warning 'Installer diagnostic collection failed; preserving original acceptance failure.' }
} finally {
    try {
        if ($ownedHost -and -not $ownedHost.HasExited) {
            Stop-Process -Id $ownedHost.Id -ErrorAction Stop
            $ownedHost.WaitForExit()
        }
        if ($credentialCreated) {
            & cmdkey.exe "/delete:$credential" | Out-Null
            if ($LASTEXITCODE -ne 0) { throw 'Exact synthetic credential cleanup failed.' }
        }
        foreach ($name in @('com.jdylanmc.pr-sniper', 'PR Sniper')) {
            if ((Get-ItemPropertyValue $runKey $name -ErrorAction SilentlyContinue) -ceq $sentinel) {
                Remove-ItemProperty $runKey $name
            }
            if ($webviewFixturePath -and (Test-Path $webviewFixturePath)) {
                Remove-ItemProperty $webviewFixturePath 'pv'
                Remove-Item $webviewFixturePath
            }
        }
    } catch {
        if ($failure) { Write-Warning 'Additional owned-fixture cleanup failure; preserving original failure.' }
        else { $failure = $_ }
    }
}
if ($failure) { throw $failure }
Get-Content (Join-Path $workspace 'acceptance.json')

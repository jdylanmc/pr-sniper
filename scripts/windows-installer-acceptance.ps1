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
    if ((Get-FileHash $app).Hash -ine $Metadata.application_sha256 -or
        [Diagnostics.FileVersionInfo]::GetVersionInfo($app).ProductVersion -cne $Metadata.version -or
        (Get-ItemPropertyValue $uninstallKey 'DisplayVersion') -cne $Metadata.version) {
        throw 'Installed bytes/version differ from this exact candidate.'
    }
    if (Get-Process -Name 'pr-sniper' -ErrorAction SilentlyContinue) { throw 'Installer silently launched the app.' }
    $script:ownedHost = Start-Process $app -WorkingDirectory $workspace -PassThru
    Start-Sleep -Seconds 5
    $script:ownedHost.Refresh()
    if ($script:ownedHost.HasExited -or $script:ownedHost.MainWindowHandle -ne [IntPtr]::Zero) {
        throw 'Installed tray-host smoke failed (exited or showed a startup window).'
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
    Invoke-Choco @('upgrade', 'pr-sniper-localtest', "--version=$($next.version)-localtest", '--pre', "--source=$($feed.FullName)")
    Assert-Installed $next (Join-Path $Upgrade 'upgrade-test-only.exe')
    Assert-Preserved
    # Prove the uninstaller preserves foreign contents, not merely a clean dir.
    $sentinel | Set-Content (Join-Path $directory 'foreign-file.txt')
    New-ItemProperty $uninstallKey 'ForeignFixture' -Value $sentinel | Out-Null
    $shell = New-Object -ComObject WScript.Shell
    $link = $shell.CreateShortcut($shortcut)
    if ($link.TargetPath -ine (Join-Path $directory 'pr-sniper.exe')) { throw 'Installed shortcut target mismatch.' }
    $link.TargetPath = $env:ComSpec
    $link.Save()
    Invoke-Choco @('uninstall', 'pr-sniper-localtest')
    Assert-Preserved
    if ((Test-Path (Join-Path $directory 'pr-sniper.exe')) -or
        (Test-Path (Join-Path $directory 'uninstall.exe')) -or
        (Get-ItemProperty $uninstallKey -Name DisplayName -ErrorAction SilentlyContinue) -or
        (Get-ItemPropertyValue $uninstallKey 'ForeignFixture') -cne $sentinel -or
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
        gui_acceptance = 'not-proven-by-host-process-smoke'
        public_distribution = $false
    } | ConvertTo-Json | Set-Content (Join-Path $workspace 'acceptance.json')
} catch {
    $failure = $_
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
        }
    } catch {
        if ($failure) { Write-Warning 'Additional owned-fixture cleanup failure; preserving original failure.' }
        else { $failure = $_ }
    }
}
if ($failure) { throw $failure }
Get-Content (Join-Path $workspace 'acceptance.json')

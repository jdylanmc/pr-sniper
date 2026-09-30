function Test-PrSniperHostReady($HostProcess, [string] $Profile) {
    $HostProcess.Refresh()
    if ($HostProcess.HasExited) { throw 'Installed host exited before owned-profile readiness.' }
    $ledger = Join-Path $Profile 'state\notifications.json'
    $diagnostics = Join-Path $Profile 'state\diagnostics.jsonl'
    if (-not (Test-Path $ledger) -or -not (Test-Path $diagnostics)) { return $false }
    $identity = Get-Content $ledger -Raw | ConvertFrom-Json
    $uuid = [guid]::Empty
    if (-not [guid]::TryParse($identity.profile_id, [ref]$uuid)) { throw 'Owned profile has invalid notification identity.' }
    # Read only complete appended records. A writer may still own the final line.
    $lines = (Get-Content $diagnostics -Raw).Split("`n")
    for ($index = 0; $index -lt $lines.Length - 1; $index++) {
        if ($lines[$index].Trim() -and ($lines[$index] | ConvertFrom-Json).event -ceq 'session_started') {
            return $true
        }
    }
    return $false
}

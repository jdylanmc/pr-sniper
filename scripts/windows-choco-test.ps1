function Invoke-PrSniperChocoTest {
    param(
        [Parameter(Mandatory)][ValidatePattern('^[a-z0-9-]+$')][string] $Name,
        [Parameter(Mandatory)][string[]] $Arguments,
        [switch] $ExpectFailure,
        [ValidateRange(1,180)][int] $TimeoutSeconds = 180
    )
    $directory = $env:PR_SNIPER_INSTALLER_DIAGNOSTICS
    if (-not $directory -or -not (Test-Path -LiteralPath $directory -PathType Container)) {
        throw 'Chocolatey test logging requires an existing owned diagnostics directory.'
    }
    $log = Join-Path $directory ("choco-$Name-" + [guid]::NewGuid() + '.log')
    $expectation = if ($ExpectFailure) { 'nonzero (injected fault)' } else { 'zero' }
    Write-Host "[Chocolatey check] $Name starting; expected exit $expectation."
    $timer = [Diagnostics.Stopwatch]::StartNew()
    & choco @Arguments --yes --no-progress --limit-output "--execution-timeout=$TimeoutSeconds" *> $log
    $code = $LASTEXITCODE
    $timer.Stop()
    if (($ExpectFailure -and $code -eq 0) -or (-not $ExpectFailure -and $code -ne 0)) {
        Get-Content -LiteralPath $log -Tail 80 | ForEach-Object { Write-Host $_ }
        throw "Chocolatey check '$Name' failed: expected $expectation, got $code. Output: $log"
    }
    Write-Host ("[PASS] Chocolatey {0}: exit {1}, {2:N1}s; expected {3}. Log: {4}" -f
        $Name, $code, $timer.Elapsed.TotalSeconds, $expectation, (Split-Path $log -Leaf))
    [pscustomobject]@{ exit_code = $code; log = $log }
}

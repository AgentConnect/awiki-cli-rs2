[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$BuildScript = Join-Path $PSScriptRoot 'build-windows.ps1'
$PreviousSource = $env:AWIKI_SOURCE_INTEGRATION
$PreviousRegistry = $env:AWIKI_RELEASE_REGISTRY
try {
    foreach ($Source in @('0', '1')) {
        $env:AWIKI_SOURCE_INTEGRATION = $Source
        $env:AWIKI_RELEASE_REGISTRY = if ($Source -eq '1') { '0' } else { '1' }
        $Output = (& $BuildScript -DryRun | Out-String).Replace('\', '/')
        $Expected = if ($Source -eq '1') { '/.artifacts/dependencies/source/target/' } else { '/target/' }
        if (-not $Output.Contains($Expected) -or -not $Output.Contains('awiki_im_core.dll')) {
            throw "Dry-run selected the wrong native output for source=$Source"
        }
        if ($Source -eq '0' -and $Output.Contains('/.artifacts/dependencies/source/')) {
            throw 'Registry mode selected a source integration artifact'
        }
    }
    $env:AWIKI_SOURCE_INTEGRATION = '1'
    $env:AWIKI_RELEASE_REGISTRY = '1'
    $Rejected = $false
    try { & $BuildScript -DryRun } catch {
        if ($_.Exception.Message -notlike '*mutually exclusive*') { throw }
        $Rejected = $true
    }
    if (-not $Rejected) { throw 'Mixed source/registry mode was not rejected' }
    Write-Output 'Windows native build entrypoint smoke PASS'
} finally {
    $env:AWIKI_SOURCE_INTEGRATION = $PreviousSource
    $env:AWIKI_RELEASE_REGISTRY = $PreviousRegistry
}

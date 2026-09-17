[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('nsis', 'msi')]
    [string]$BundleKind
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$bundleDirectory = Join-Path $repositoryRoot "src-tauri\target\release\bundle\$BundleKind"
$pattern = if ($BundleKind -eq 'nsis') { '*.exe' } else { '*.msi' }
$packages = @(Get-ChildItem -LiteralPath $bundleDirectory -Filter $pattern -File)
if ($packages.Count -ne 1) {
    throw "Expected one $BundleKind package in $bundleDirectory, found $($packages.Count)"
}

function Assert-WorkerArtifact {
    param([Parameter(Mandatory = $true)][string]$Root)

    $directories = @(Get-ChildItem -LiteralPath $Root -Filter manifest.json -File -Recurse |
        Where-Object {
            $_.Directory.Name -eq 'current' -and
            $_.Directory.Parent.Name -eq 'wsl-worker' -and
            (Test-Path -LiteralPath (Join-Path $_.Directory.FullName 'worker') -PathType Leaf)
        } |
        ForEach-Object { $_.Directory.FullName })
    if ($directories.Count -ne 1) {
        throw "Expected one bundled WSL Worker resource, found $($directories.Count)"
    }
    $global:LASTEXITCODE = 0
    & node (Join-Path $repositoryRoot 'scripts\prepare-wsl-worker.mjs') --verify $directories[0]
    if ($LASTEXITCODE -ne 0) {
        throw 'Bundled WSL Worker artifact verification failed'
    }
}

$temporaryRoot = Join-Path ([IO.Path]::GetTempPath()) "skill-deck-worker-bundle-$([Guid]::NewGuid().ToString('N'))"
try {
    if ($BundleKind -eq 'msi') {
        New-Item -ItemType Directory -Path $temporaryRoot | Out-Null
        $msiArguments = @(
            '/a',
            "`"$($packages[0].FullName)`"",
            '/qn',
            "TARGETDIR=`"$temporaryRoot`""
        )
        $process = Start-Process -FilePath msiexec.exe -ArgumentList $msiArguments -Wait -PassThru
        if ($process.ExitCode -ne 0) {
            throw "MSI administrative extraction failed with exit code $($process.ExitCode)"
        }
        Assert-WorkerArtifact -Root $temporaryRoot
    }
    else {
        $process = Start-Process -FilePath $packages[0].FullName -ArgumentList @('/S', "/D=$temporaryRoot") -Wait -PassThru
        if ($process.ExitCode -ne 0) {
            throw "NSIS installation failed with exit code $($process.ExitCode)"
        }
        Assert-WorkerArtifact -Root $temporaryRoot
    }
}
finally {
    if ($BundleKind -eq 'nsis') {
        $uninstaller = Join-Path $temporaryRoot 'uninstall.exe'
        if (Test-Path -LiteralPath $uninstaller -PathType Leaf) {
            Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait | Out-Null
        }
    }
    Remove-Item -LiteralPath $temporaryRoot -Recurse -Force -ErrorAction SilentlyContinue
}

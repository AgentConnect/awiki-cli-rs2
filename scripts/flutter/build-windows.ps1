[CmdletBinding()]
param(
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$RootDir = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$Target = if ([string]::IsNullOrWhiteSpace($env:AWIKI_IM_CORE_WINDOWS_TARGET)) {
    'x86_64-pc-windows-msvc'
} else {
    $env:AWIKI_IM_CORE_WINDOWS_TARGET.Trim()
}
$Toolchain = if ([string]::IsNullOrWhiteSpace($env:AWIKI_IM_CORE_RUST_TOOLCHAIN)) {
    '1.88.0'
} else {
    $env:AWIKI_IM_CORE_RUST_TOOLCHAIN.Trim()
}
$Features = 'blocking,sqlite,http,windows'
$SourceDll = Join-Path $RootDir "target\$Target\release\awiki_im_core.dll"
$DestinationDir = Join-Path $RootDir 'packages\awiki_im_core\windows\bin'
$DestinationDll = Join-Path $DestinationDir 'awiki_im_core.dll'
$GeneratedDart = Join-Path $RootDir 'packages\awiki_im_core\lib\src\generated\frb_generated.dart'

if ($DryRun) {
    Write-Output "Would run: rustup target add --toolchain $Toolchain $Target"
    Write-Output "Would run: cargo +$Toolchain build -p im-core-dart --release --locked --target $Target --no-default-features --features $Features"
    Write-Output "Would verify and copy: $SourceDll -> $DestinationDll"
    return
}

if ([System.Environment]::OSVersion.Platform -ne [System.PlatformID]::Win32NT) {
    throw 'The awiki_im_core Windows native build must run on Windows.'
}
if ($Target -ne 'x86_64-pc-windows-msvc') {
    throw "Unsupported awiki_im_core Windows target: $Target. Only x86_64-pc-windows-msvc is supported."
}

foreach ($CommandName in @('rustup', 'cargo')) {
    if ($null -eq (Get-Command $CommandName -ErrorAction SilentlyContinue)) {
        throw "$CommandName is required to build awiki_im_core for Windows."
    }
}

Push-Location $RootDir
try {
    & rustup target add --toolchain $Toolchain $Target
    if ($LASTEXITCODE -ne 0) {
        throw "rustup target add failed with exit code $LASTEXITCODE."
    }

    & cargo "+$Toolchain" build `
        -p im-core-dart `
        --release `
        --locked `
        --target $Target `
        --no-default-features `
        --features $Features
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed with exit code $LASTEXITCODE."
    }
} finally {
    Pop-Location
}

if (-not (Test-Path -LiteralPath $SourceDll -PathType Leaf)) {
    throw "Built DLL was not found: $SourceDll"
}

New-Item -ItemType Directory -Path $DestinationDir -Force | Out-Null
Copy-Item -LiteralPath $SourceDll -Destination $DestinationDll -Force

function Get-PeMachine {
    param([Parameter(Mandatory = $true)][string]$Path)

    $Stream = [System.IO.File]::OpenRead($Path)
    $Reader = $null
    try {
        $Reader = [System.IO.BinaryReader]::new($Stream)
        if ($Reader.ReadUInt16() -ne 0x5A4D) {
            throw "$Path does not have a DOS MZ header."
        }
        $Stream.Position = 0x3C
        $PeOffset = $Reader.ReadInt32()
        $Stream.Position = $PeOffset
        if ($Reader.ReadUInt32() -ne 0x00004550) {
            throw "$Path does not have a PE header."
        }
        return $Reader.ReadUInt16()
    } finally {
        if ($null -ne $Reader) {
            $Reader.Dispose()
        } else {
            $Stream.Dispose()
        }
    }
}

$Machine = Get-PeMachine -Path $DestinationDll
if ($Machine -ne 0x8664) {
    throw ('Expected an x64 PE DLL (machine 0x8664), got 0x{0:X4}.' -f $Machine)
}

function Resolve-DumpbinPath {
    $Command = Get-Command dumpbin.exe -ErrorAction SilentlyContinue
    if ($null -ne $Command) {
        return $Command.Source
    }

    $VsWhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path -LiteralPath $VsWhere -PathType Leaf)) {
        throw 'dumpbin.exe is unavailable and vswhere.exe could not be found.'
    }
    $InstallationPath = & $VsWhere `
        -latest `
        -products '*' `
        -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
        -property installationPath | Select-Object -First 1
    if ([string]::IsNullOrWhiteSpace($InstallationPath)) {
        throw 'Visual Studio with the MSVC x64 tools is required.'
    }
    $InstallationPath = $InstallationPath.Trim()
    $Candidates = Get-ChildItem `
        -Path (Join-Path $InstallationPath 'VC\Tools\MSVC\*\bin\Hostx64\x64\dumpbin.exe') `
        -File `
        -ErrorAction SilentlyContinue | Sort-Object FullName -Descending
    if ($null -eq $Candidates -or $Candidates.Count -eq 0) {
        throw "dumpbin.exe was not found under $InstallationPath"
    }
    return $Candidates[0].FullName
}

$DumpbinPath = Resolve-DumpbinPath
$ExportLines = & $DumpbinPath /nologo /exports $DestinationDll
if ($LASTEXITCODE -ne 0) {
    throw "dumpbin /exports failed with exit code $LASTEXITCODE."
}
$RequiredExports = @(
    'frb_get_rust_content_hash',
    'frb_pde_ffi_dispatcher_primary',
    'frb_pde_ffi_dispatcher_sync',
    'frb_dart_fn_deliver_output'
)
foreach ($ExportName in $RequiredExports) {
    $ExportPattern = '^\s+\d+\s+[0-9A-Fa-f]+\s+[0-9A-Fa-f]+\s+' +
        [System.Text.RegularExpressions.Regex]::Escape($ExportName) + '\s*$'
    if ($null -eq ($ExportLines | Select-String -Pattern $ExportPattern)) {
        throw "Required flutter_rust_bridge export is missing: $ExportName"
    }
}

$GeneratedSource = Get-Content -LiteralPath $GeneratedDart -Raw
$HashMatch = [System.Text.RegularExpressions.Regex]::Match(
    $GeneratedSource,
    'int get rustContentHash\s*=>\s*(-?\d+);'
)
if (-not $HashMatch.Success) {
    throw "Could not read the generated Dart content hash from $GeneratedDart"
}
$ExpectedContentHash = [int]::Parse(
    $HashMatch.Groups[1].Value,
    [System.Globalization.CultureInfo]::InvariantCulture
)
$NativeProbeSource = @"
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;

public static class AwikiImCoreNativeBuildProbe
{
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr LoadLibraryW(string path);

    [DllImport("kernel32.dll", CharSet = CharSet.Ansi, SetLastError = true)]
    private static extern IntPtr GetProcAddress(IntPtr module, string name);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool FreeLibrary(IntPtr module);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate int ContentHashDelegate();

    public static int ReadContentHash(string path)
    {
        IntPtr module = LoadLibraryW(path);
        if (module == IntPtr.Zero)
        {
            throw new Win32Exception(Marshal.GetLastWin32Error());
        }
        try
        {
            IntPtr function = GetProcAddress(module, "frb_get_rust_content_hash");
            if (function == IntPtr.Zero)
            {
                throw new Win32Exception(Marshal.GetLastWin32Error());
            }
            var callback = (ContentHashDelegate)Marshal.GetDelegateForFunctionPointer(
                function,
                typeof(ContentHashDelegate)
            );
            return callback();
        }
        finally
        {
            FreeLibrary(module);
        }
    }
}
"@
Add-Type -TypeDefinition $NativeProbeSource
$ActualContentHash = [AwikiImCoreNativeBuildProbe]::ReadContentHash($DestinationDll)
if ($ActualContentHash -ne $ExpectedContentHash) {
    throw "FRB content hash mismatch: Dart=$ExpectedContentHash, DLL=$ActualContentHash"
}

$Artifact = Get-Item -LiteralPath $DestinationDll
$Sha256 = (Get-FileHash -LiteralPath $DestinationDll -Algorithm SHA256).Hash.ToLowerInvariant()
Write-Output "Windows native SDK artifact: $DestinationDll"
Write-Output "Architecture: x86_64-pc-windows-msvc"
Write-Output "FRB content hash: $ActualContentHash"
Write-Output "Size: $($Artifact.Length) bytes"
Write-Output "SHA-256: $Sha256"

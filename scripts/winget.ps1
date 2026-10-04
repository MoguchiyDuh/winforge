param([switch]$Force)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$family = 'Microsoft.DesktopAppInstaller_8wekyb3d8bbwe'
$isSystem = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value -eq 'S-1-5-18'

function Find-Winget {
    $alias = Join-Path $env:LOCALAPPDATA 'Microsoft\WindowsApps\winget.exe'
    if (-not $isSystem -and (Test-Path -LiteralPath $alias)) { return $alias }
    $pkg = Get-AppxPackage -Name 'Microsoft.DesktopAppInstaller' -ErrorAction SilentlyContinue |
        Sort-Object Version -Descending | Select-Object -First 1
    if ($pkg) {
        $exe = Join-Path $pkg.InstallLocation 'winget.exe'
        if (Test-Path -LiteralPath $exe) { return $exe }
    }
    return $null
}

function Test-Winget {
    $exe = Find-Winget
    if (-not $exe) { return $false }
    try {
        $null = & $exe --version 2>&1
        return $LASTEXITCODE -eq 0
    } catch {
        return $false
    }
}

if (-not $Force -and (Test-Winget)) {
    Write-Output 'winget already works'
    exit 0
}

try {
    Write-Output 'installing NuGet provider and Microsoft.WinGet.Client'
    Install-PackageProvider -Name NuGet -MinimumVersion 2.8.5.201 -Force -ForceBootstrap -Scope AllUsers | Out-Null
    if ((Get-PSRepository -Name PSGallery).InstallationPolicy -ne 'Trusted') {
        Set-PSRepository -Name PSGallery -InstallationPolicy Trusted
    }
    Install-Module -Name Microsoft.WinGet.Client -Repository PSGallery -Scope AllUsers -Force -AllowClobber
    Import-Module -Name Microsoft.WinGet.Client -Force
    Write-Output 'running Repair-WinGetPackageManager -AllUsers -Force -Latest'
    Repair-WinGetPackageManager -AllUsers -Force -Latest
} catch {
    Write-Output "repair failed: $_"
    exit 1
}

if (-not $isSystem) {
    try {
        Add-AppxPackage -RegisterByFamilyName -MainPackage $family -ErrorAction Stop
    } catch {
        Write-Output "register for current user failed: $_"
    }
}

$exe = Find-Winget
if (-not $exe) {
    Write-Output 'winget.exe not found after repair'
    exit 1
}
if ($Force) {
    Write-Output 'resetting winget sources'
    $null = & $exe source reset --force --disable-interactivity 2>&1
}
$null = & $exe source update --disable-interactivity 2>&1
$version = & $exe --version 2>&1
if ($LASTEXITCODE -ne 0) {
    Write-Output "winget still broken (exit $LASTEXITCODE)"
    exit 1
}
Write-Output "winget $version ready"
exit 0

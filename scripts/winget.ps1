param([switch]$Force)
$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
if (-not $Force -and (Get-Command winget -ErrorAction SilentlyContinue)) { exit 0 }
try {
    Install-PackageProvider -Name NuGet -Force | Out-Null
    Install-Module -Name Microsoft.WinGet.Client -Force
    Repair-WinGetPackageManager -AllUsers
} catch {
    Write-Error $_
    exit 1
}
exit 0

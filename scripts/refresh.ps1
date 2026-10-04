$ErrorActionPreference = 'SilentlyContinue'
$signature = @'
[DllImport("user32.dll", EntryPoint="SystemParametersInfoW", SetLastError=true)]
public static extern bool SPI_Value(uint a, uint b, IntPtr c, uint d);
[DllImport("user32.dll", EntryPoint="SystemParametersInfoW", SetLastError=true)]
public static extern bool SPI_Mouse(uint a, uint b, [In] int[] c, uint d);
[DllImport("user32.dll", EntryPoint="SystemParametersInfoW", SetLastError=true)]
public static extern bool SPI_Sticky(uint a, uint b, ref STICKYKEYS c, uint d);
[DllImport("user32.dll", EntryPoint="SystemParametersInfoW", CharSet=CharSet.Unicode)]
public static extern bool SPI_Str(uint a, uint b, string c, uint d);
[DllImport("user32.dll", EntryPoint="RegisterWindowMessageW", CharSet=CharSet.Unicode, SetLastError=true)]
public static extern uint RegisterWindowMsg(string lpString);
[DllImport("user32.dll", EntryPoint="SendMessageTimeoutW", CharSet=CharSet.Unicode, SetLastError=true)]
public static extern IntPtr SendMsgTimeoutS(IntPtr h, uint m, IntPtr w, string l, uint f, uint t, out IntPtr r);
[DllImport("user32.dll", EntryPoint="SendMessageTimeoutW", SetLastError=true)]
public static extern IntPtr SendMsgTimeoutP(IntPtr h, uint m, IntPtr w, IntPtr l, uint f, uint t, out IntPtr r);
public struct STICKYKEYS { public uint cbSize; public uint dwFlags; }
'@
try { Add-Type -Namespace WF -Name Refresh -MemberDefinition $signature -ErrorAction Stop } catch { }
function Get-Num([string]$path, [string]$name) {
    $raw = (Get-ItemProperty -LiteralPath $path -Name $name -ErrorAction SilentlyContinue).$name
    $num = 0
    if ($null -ne $raw -and [int]::TryParse([string]$raw, [ref]$num)) { return $num }
    return $null
}
try {
    $kbd = 'HKCU:\Control Panel\Keyboard'
    $mouse = 'HKCU:\Control Panel\Mouse'
    $delay = Get-Num $kbd 'KeyboardDelay'
    if ($null -ne $delay) { [void][WF.Refresh]::SPI_Value(0x0017, [uint32]$delay, [IntPtr]::Zero, 2) }
    $speed = Get-Num $kbd 'KeyboardSpeed'
    if ($null -ne $speed) { [void][WF.Refresh]::SPI_Value(0x000B, [uint32]$speed, [IntPtr]::Zero, 2) }
    $sens = Get-Num $mouse 'MouseSensitivity'
    if ($null -ne $sens) { [void][WF.Refresh]::SPI_Value(0x0071, 0, [IntPtr]$sens, 2) }
    $t1 = Get-Num $mouse 'MouseThreshold1'
    $t2 = Get-Num $mouse 'MouseThreshold2'
    $accel = Get-Num $mouse 'MouseSpeed'
    if ($null -ne $t1 -and $null -ne $t2 -and $null -ne $accel) {
        [void][WF.Refresh]::SPI_Mouse(0x0004, 0, [int[]]@($t1, $t2, $accel), 2)
    }
    $wp = (Get-ItemProperty -Path 'HKCU:\Control Panel\Desktop' -Name WallPaper -ErrorAction SilentlyContinue).WallPaper
    if ($wp) { [void][WF.Refresh]::SPI_Str(0x0014, 0, $wp, 2) }
    [void][WF.Refresh]::SPI_Value(0x0057, 0, [IntPtr]::Zero, 0)
    $flags = Get-Num 'HKCU:\Control Panel\Accessibility\StickyKeys' 'Flags'
    if ($null -ne $flags) {
        $sk = New-Object WF.Refresh+STICKYKEYS
        $sk.cbSize = [uint32][Runtime.InteropServices.Marshal]::SizeOf([type][WF.Refresh+STICKYKEYS])
        $sk.dwFlags = [uint32]$flags
        [void][WF.Refresh]::SPI_Sticky(0x003B, $sk.cbSize, [ref]$sk, 2)
    }
    $res = [IntPtr]::Zero
    $imm = [WF.Refresh]::RegisterWindowMsg('ImmersiveColorSet')
    [void][WF.Refresh]::SendMsgTimeoutS([IntPtr]0xffff, 0x001A, [IntPtr]::Zero, 'ImmersiveColorSet', 2, 3000, [ref]$res)
    [void][WF.Refresh]::SendMsgTimeoutP([IntPtr]0xffff, $imm, [IntPtr]::Zero, [IntPtr]::Zero, 2, 3000, [ref]$res)
    [void][WF.Refresh]::SendMsgTimeoutP([IntPtr]0xffff, 0x0320, [IntPtr]::Zero, [IntPtr]::Zero, 2, 3000, [ref]$res)
} catch { }
Stop-Process -Name explorer -Force
Start-Sleep -Seconds 2
if (-not (Get-Process -Name explorer -ErrorAction SilentlyContinue)) {
    Start-Process -FilePath 'explorer.exe'
}
exit 0

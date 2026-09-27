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
try {
    [WF.Refresh]::SPI_Value(0x0017, 0, [IntPtr]::Zero, 3) | Out-Null
    [WF.Refresh]::SPI_Value(0x000B, 31, [IntPtr]::Zero, 3) | Out-Null
    [WF.Refresh]::SPI_Value(0x0071, 0, [IntPtr]7, 3) | Out-Null
    [WF.Refresh]::SPI_Mouse(0x0004, 0, [int[]]@(0, 0, 0), 3) | Out-Null
    $wp = (Get-ItemProperty -Path 'HKCU:\Control Panel\Desktop' -Name WallPaper -ErrorAction SilentlyContinue).WallPaper
    if ($wp) { [void][WF.Refresh]::SPI_Str(0x0014, 0, $wp, 3) }
    [void][WF.Refresh]::SPI_Value(0x0057, 0, [IntPtr]::Zero, 0)
    $sk = New-Object WF.Refresh+STICKYKEYS
    $sk.cbSize = [uint32][Runtime.InteropServices.Marshal]::SizeOf([type][WF.Refresh+STICKYKEYS])
    $sk.dwFlags = 506
    [WF.Refresh]::SPI_Sticky(0x003B, $sk.cbSize, [ref]$sk, 3) | Out-Null
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

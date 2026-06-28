Add-Type -AssemblyName System.Drawing
$sig = @'
[DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hWnd, IntPtr hdcBlt, uint nFlags);
[DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr hWnd, out RECT r);
[DllImport("user32.dll")] public static extern bool IsIconic(IntPtr hWnd);
[DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int n);
public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
'@
$t = Add-Type -MemberDefinition $sig -Name W2 -Namespace U2 -PassThru
$win = $t | Where-Object { $_.Name -eq 'W2' } | Select-Object -First 1
$proc = Get-Process sigma-file-manager -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if (-not $proc) { Write-Output "NO_WINDOW_HANDLE"; exit }
$h = $proc.MainWindowHandle
if ($win::IsIconic($h)) { $win::ShowWindow($h, 9) | Out-Null; Start-Sleep -Milliseconds 600 }
$r = New-Object U2.W2+RECT
$win::GetClientRect($h, [ref]$r) | Out-Null
$w = $r.Right - $r.Left
$ht = $r.Bottom - $r.Top
if ($w -le 0 -or $ht -le 0) { Write-Output "CLIENT_RECT_EMPTY ${w}x${ht}"; exit }
$bmp = New-Object System.Drawing.Bitmap $w, $ht
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
$ok = $win::PrintWindow($h, $hdc, 2)
$g.ReleaseHdc($hdc)
$out = "E:\ai\Projects\Meridian\meridian_printwindow.png"
$bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
Write-Output "SAVED $out ${w}x${ht} printwindow_ok=$ok"

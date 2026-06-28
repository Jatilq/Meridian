Add-Type -AssemblyName System.Windows.Forms,System.Drawing
$proc = Get-Process sigma-file-manager -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if (-not $proc) { Write-Output "NO_WINDOW_HANDLE"; exit }
$sig = @'
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
[DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int n);
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT r);
public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
'@
$t = Add-Type -MemberDefinition $sig -Name Win -Namespace U -PassThru
$win = $t | Where-Object { $_.Name -eq 'Win' } | Select-Object -First 1
$h = $proc.MainWindowHandle
$win::ShowWindow($h, 9) | Out-Null
$win::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 900
$r = New-Object U.Win+RECT
$win::GetWindowRect($h, [ref]$r) | Out-Null
$width = $r.Right - $r.Left
$height = $r.Bottom - $r.Top
if ($width -le 0 -or $height -le 0) { Write-Output "BAD_RECT ${width}x${height}"; exit }
$bmp = New-Object System.Drawing.Bitmap $width, $height
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($r.Left, $r.Top, 0, 0, $bmp.Size)
$out = "E:\ai\Projects\Meridian\meridian_capture.png"
$bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
Write-Output "SAVED $out ${width}x${height}"

# Generates xm-control.ico (headphones, 16/24/32/48/64/256 px, PNG-compressed entries) with System.Drawing.
# Usage: powershell -File make_icon.ps1 [-Out xm-control.ico] [-Preview preview-256.png] [-Sheet sheet.png]
param(
    [string]$Out = (Join-Path $PSScriptRoot 'xm-control.ico'),
    [string]$Preview,
    [string]$Sheet
)
Add-Type -AssemblyName System.Drawing
$sizes = 16, 24, 32, 48, 64, 256

function New-Round([float]$x, [float]$y, [float]$w, [float]$h, [float]$r) {
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = 2 * $r
    $p.AddArc($x, $y, $d, $d, 180, 90); $p.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
    $p.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90); $p.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
    $p.CloseFigure(); $p
}

function Render([int]$s) {
    $bmp = New-Object System.Drawing.Bitmap $s, $s, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'AntiAlias'; $g.PixelOffsetMode = 'HighQuality'; $g.CompositingQuality = 'HighQuality'
    $u = $s / 100.0
    $thick = if ($s -le 16) { 1.5 } elseif ($s -le 24) { 1.35 } elseif ($s -le 32) { 1.15 } else { 1.0 }   # chunkier shapes at tiny sizes
    $ol = [Math]::Max(1.0, 3.0 * $u)                                           # light outline so it reads on dark taskbars
    $dark = [System.Drawing.Color]::FromArgb(255, 31, 41, 55)
    $teal = [System.Drawing.Color]::FromArgb(255, 20, 184, 166)
    $white = [System.Drawing.Color]::FromArgb(235, 255, 255, 255)
    $bw = 10 * $u * $thick
    $cw = 22 * $u * [Math]::Min($thick, 1.2)
    $r = 34 * $u
    $cx = 50 * $u; $cy = 54 * $u
    $arc = New-Object System.Drawing.RectangleF ($cx - $r), ($cy - $r), (2 * $r), (2 * $r)
    $ch = 40 * $u; $cyTop = 50 * $u
    $lx = 6 * $u; $rx = 94 * $u - $cw
    $cups = @((New-Round $lx $cyTop $cw $ch (8 * $u)), (New-Round $rx $cyTop $cw $ch (8 * $u)))
    # pass 1: outline, pass 2: fill
    $pen = New-Object System.Drawing.Pen $white, ($bw + 2 * $ol); $pen.StartCap = 'Round'; $pen.EndCap = 'Round'
    $g.DrawArc($pen, $arc, 180, 180)
    $wb = New-Object System.Drawing.SolidBrush $white
    $wp = New-Object System.Drawing.Pen $white, (2 * $ol); $wp.LineJoin = 'Round'
    foreach ($c in $cups) { $g.DrawPath($wp, $c); $g.FillPath($wb, $c) }
    $pen2 = New-Object System.Drawing.Pen $dark, $bw; $pen2.StartCap = 'Round'; $pen2.EndCap = 'Round'
    $g.DrawArc($pen2, $arc, 180, 180)
    $db = New-Object System.Drawing.SolidBrush $dark
    foreach ($c in $cups) { $g.FillPath($db, $c) }
    # teal cushion on the inner face of each cup
    $tb = New-Object System.Drawing.SolidBrush $teal
    $cu = [Math]::Max(1.0, $cw * 0.42)
    $g.FillPath($tb, (New-Round ($lx + $cw - $cu) ($cyTop + 4 * $u) $cu ($ch - 8 * $u) ([Math]::Min($cu / 2, 6 * $u))))
    $g.FillPath($tb, (New-Round $rx ($cyTop + 4 * $u) $cu ($ch - 8 * $u) ([Math]::Min($cu / 2, 6 * $u))))
    $g.Dispose(); $bmp
}

$bitmaps = @{}
$pngs = foreach ($s in $sizes) {
    $b = Render $s; $bitmaps[$s] = $b
    $ms = New-Object System.IO.MemoryStream; $b.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png); , $ms.ToArray()
}
$fs = [System.IO.File]::Create($Out); $w = New-Object System.IO.BinaryWriter $fs
$w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$sizes.Count)
$off = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $s = $sizes[$i]; $dim = if ($s -ge 256) { 0 } else { $s }
    $w.Write([byte]$dim); $w.Write([byte]$dim); $w.Write([byte]0); $w.Write([byte]0)
    $w.Write([uint16]1); $w.Write([uint16]32); $w.Write([uint32]$pngs[$i].Length); $w.Write([uint32]$off)
    $off += $pngs[$i].Length
}
foreach ($p in $pngs) { $w.Write([byte[]]$p) }
$w.Close(); $fs.Close()

if ($Preview) { $bitmaps[256].Save($Preview, [System.Drawing.Imaging.ImageFormat]::Png) }
if ($Sheet) {   # contact sheet on light and dark backgrounds: every size 1:1, then 16/32/48 scaled up 8x/4x/4x
    $sh = New-Object System.Drawing.Bitmap 1100, 640
    $g = [System.Drawing.Graphics]::FromImage($sh); $g.InterpolationMode = 'NearestNeighbor'; $g.PixelOffsetMode = 'Half'
    $g.Clear([System.Drawing.Color]::FromArgb(255, 240, 240, 240))
    $g.FillRectangle((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 32, 32, 32))), 0, 320, 1100, 320)
    foreach ($row in 0, 320) {
        $x = 10
        foreach ($s in $sizes) { $g.DrawImage($bitmaps[$s], $x, $row + 10, $s, $s); $x += $s + 14 }
        $g.DrawImage($bitmaps[16], 10, $row + 140, 128, 128)
        $g.DrawImage($bitmaps[32], 160, $row + 140, 128, 128)
        $g.DrawImage($bitmaps[48], 310, $row + 140, 144, 144)
    }
    $sh.Save($Sheet, [System.Drawing.Imaging.ImageFormat]::Png)
}

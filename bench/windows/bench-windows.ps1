<#
.SYNOPSIS
  XQ 壓測 Windows 版：GPUI vs egui（wry 已淘汰，不比）：主視窗、5 萬列、重圖表、高頻 1k/10k/50k/100k/爆量、多視窗（1、4 個跨螢幕）、
  網頁版一頁嵌多個元件（web：用 Chrome／Edge 開 web\egui、web\gpui）。
.DESCRIPTION
  與 Linux 版 scripts/bench-main-window.sh、bench-ticks.sh、bench-multi.sh 同情境、同量法（3 輪取中位數）。
  執行檔來源依序：-BinDir、本資料夾 bin\、GitHub release「windows-bench-latest」自動下載（兩個 repo 都公開，不用登入）。
  結果：results\<時間>-<電腦名>\ 內的 results.json、summary.md（中文、結論在前）、log、截圖，以及同名 .zip。
.EXAMPLE
  .\bench-windows.ps1                 # 全部（約 75 分鐘）
  .\bench-windows.ps1 -Quick          # 快速版（1 輪、少幾個情境，約 20 分鐘）
  .\bench-windows.ps1 -Scenarios ticks -Ticks s10k,s100k -Apps gpui,egui
  .\bench-windows.ps1 -Scenarios web       # 只跑網頁版（約 8 分鐘）
#>
[CmdletBinding()]
param(
    [string]$BinDir = "",
    [int]$Runs = 3,
    [string[]]$Scenarios = @("main", "stress", "heavy", "ticks", "multi", "web"),
    [string[]]$Ticks = @("s1k", "s10k", "s50k", "s100k", "burst"),
    [string[]]$Apps = @("gpui", "gpui-quads", "egui", "egui-wgpu"),
    [string[]]$Windows = @("1", "4"),
    [switch]$Quick,
    [switch]$Update,
    [switch]$NoDownload,
    [string]$SrcRoot = "",
    [switch]$Build,
    [string]$OutDir = "",
    [string]$WebDir = "",
    [switch]$NoZip,
    [string]$Owner = "tina-moneydj",
    [string]$Tag = "windows-bench-latest"
)

Set-StrictMode -Version 2
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch { }

# 逗號分隔也接受（run-bench.cmd -Apps gpui,egui 從 cmd 傳進來是一個字串）
function Split-Arg($a) { return @($a | ForEach-Object { "$_" -split "," } | ForEach-Object { $_.Trim() } | Where-Object { $_ }) }
$Scenarios = Split-Arg $Scenarios; $Ticks = Split-Arg $Ticks; $Apps = Split-Arg $Apps
$Windows = @(Split-Arg $Windows | ForEach-Object { [int]$_ })
if ($Quick) {
    if (-not $PSBoundParameters.ContainsKey("Runs")) { $Runs = 1 }
    if (-not $PSBoundParameters.ContainsKey("Ticks")) { $Ticks = @("s10k", "s100k", "burst") }
}

# ---------------------------------------------------------------- 常數（同 Linux 版）
$Addr = "127.0.0.1:47631"; $Port = 47631
$W = 1200; $H = 720                       # 主視窗內容區（邏輯像素，乘上 DPI 縮放）
$T_IDLE = 10.0; $T_CPU0 = 30.0; $T_END = 60.0
$DUR = 30; $SETTLE = 3; $WARMUP = 3; $TAIL = 4
# 重圖表 7 段（app 端 chart.rs／zstress.rs：每段 10 秒，zshot 3 秒，共約 67 秒＋暖機 3 秒）
$HEAVY_PHASES = @("ticks", "crosshair", "pan", "drag", "select", "reorder", "zshot")
$TICK_ARGS = @{ s1k = "--rate 1000"; s10k = "--rate 10000"; s50k = "--rate 50000"; s100k = "--rate 100000"; burst = "--burst 150000,3,5000" }
# 報價表 2330／2317 兩列的點擊座標（內容區邏輯像素，同 Linux 1200×720 校正值）；可用環境變數覆寫
function EnvInt($k, $d) { $v = [Environment]::GetEnvironmentVariable($k); if ($v) { return [int]$v } return $d }
$CLICK = @{
    gpui = @{ "2330" = @(30, (EnvInt "GPUI_ROW0_Y" 462)); "2317" = @(30, (EnvInt "GPUI_ROW1_Y" 484)) }
    egui = @{ "2330" = @(30, (EnvInt "EGUI_ROW0_Y" 462)); "2317" = @(30, (EnvInt "EGUI_ROW1_Y" 484)) }
}
$CLICK["egui-wgpu"] = $CLICK.egui; $CLICK["gpui-quads"] = $CLICK.gpui
$WATCH = @("2330","2317","2454","0050","2303","2881","2882","2308","2412","2382","3711","2891","2886","1301","1303",
           "2002","3008","2357","2603","2609","3231","6505","2884","2892","5880","2207","1216","2379","3034","2345",
           "AAPL","TSLA","NVDA")
$EXES = [ordered]@{
    demofeed = @{ file = "xq-demofeed.exe";  repo = "gpui-xq-demo"; private = $false }
    gpui     = @{ file = "gpui-xq-demo.exe"; repo = "gpui-xq-demo"; private = $false }
    tickgen  = @{ file = "xq-tickgen.exe";   repo = "gpui-xq-demo"; private = $false }
    egui     = @{ file = "egui-xq-demo.exe"; repo = "egui-xq-demo"; private = $false }
}
# 同一個 exe 用不同 renderer 跑的 app（XQ_RENDERER）：egui＝glow／OpenGL，egui-wgpu＝wgpu（Windows 上走 DX12／Vulkan）
$APP_BIN = @{ gpui = "gpui"; "gpui-quads" = "gpui"; egui = "egui"; "egui-wgpu" = "egui" }
# gpui-quads＝同一個 GPUI exe 打開 XQ_DRAW_QUADS=1（畫線層橫平豎直的圖形改用 quad 畫）；只影響畫線，所以只跑重圖表，其他情境略過
$HEAVY_ONLY = @("gpui-quads")
function Runs-In($a, $sc) { return ($sc -eq "heavy") -or ($HEAVY_ONLY -notcontains $a) }

# ---------------------------------------------------------------- 原生輔助
$csPath = Join-Path $PSScriptRoot "XqBench.cs"
if (-not ("XqBench" -as [type])) {
    $src = [IO.File]::ReadAllText($csPath, [Text.Encoding]::UTF8)
    if ($PSVersionTable.PSEdition -eq "Core") { Add-Type -TypeDefinition $src -Language CSharp }
    else { Add-Type -TypeDefinition $src -Language CSharp -ReferencedAssemblies System.Core }
}
Add-Type -AssemblyName System.Drawing
$DpiMode = [XqBench]::SetDpiAware()

# ---------------------------------------------------------------- 小工具
$Script:LogLines = New-Object System.Collections.Generic.List[string]
function Say($msg, $color = "Gray") {
    $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $msg
    Write-Host $line -ForegroundColor $color
    $Script:LogLines.Add($line)
}
function R1($v) { if ($null -eq $v) { return $null } return [math]::Round([double]$v, 1) }
function Med($xs) {
    $v = @($xs | Where-Object { $null -ne $_ } | ForEach-Object { [double]$_ } | Sort-Object)
    if ($v.Count -eq 0) { return $null }
    if ($v.Count % 2 -eq 1) { return $v[[int](($v.Count - 1) / 2)] }
    return ($v[[int]($v.Count / 2) - 1] + $v[[int]($v.Count / 2)]) / 2
}
function Pct($xs, $q) {
    $v = @($xs | Where-Object { $null -ne $_ } | ForEach-Object { [double]$_ } | Sort-Object)
    if ($v.Count -eq 0) { return $null }
    $i = [int][math]::Round($q / 100.0 * ($v.Count - 1))
    $i = [math]::Min($v.Count - 1, [math]::Max(0, $i))
    return $v[$i]
}
function MaxOf($xs) { $v = @($xs | Where-Object { $null -ne $_ } | ForEach-Object { [double]$_ }); if ($v.Count -eq 0) { return $null } return ($v | Measure-Object -Maximum).Maximum }
function MinOf($xs) { $v = @($xs | Where-Object { $null -ne $_ } | ForEach-Object { [double]$_ }); if ($v.Count -eq 0) { return $null } return ($v | Measure-Object -Minimum).Minimum }
function Get-P($o, $k) {
    if ($null -eq $o) { return $null }
    if ($o -is [System.Collections.IDictionary]) { return $o[$k] }
    $p = $o.PSObject.Properties[$k]; if ($p) { return $p.Value } return $null
}
function Sleep-Until([double]$t) { while ([XqBench]::Now() -lt $t) { Start-Sleep -Milliseconds 20 } }
function Port-Busy {
    try { $l = New-Object System.Net.Sockets.TcpListener([System.Net.IPAddress]::Loopback, $Port); $l.Start(); $l.Stop(); return $false }
    catch { return $true }
}
function Wait-Port($timeoutS) {
    $end = [XqBench]::Now() + $timeoutS
    while ([XqBench]::Now() -lt $end) {
        try { $c = New-Object System.Net.Sockets.TcpClient; $c.Connect("127.0.0.1", $Port); $c.Close(); return $true } catch { Start-Sleep -Milliseconds 100 }
    }
    return $false
}

# ---------------------------------------------------------------- 執行檔
function Download-Asset($repo, $file, $dest, $private) {
    $url = "https://github.com/$Owner/$repo/releases/download/$Tag/$file"
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    if (-not $private) {
        Say "下載 $url"
        Invoke-WebRequest -Uri $url -OutFile $dest -UseBasicParsing
        return $true
    }
    $gh = Get-Command gh -ErrorAction SilentlyContinue
    if ($gh) {
        Say "用 gh 下載 $Owner/$repo $Tag/$file"
        $ea = $ErrorActionPreference; $ErrorActionPreference = "Continue"
        & gh release download $Tag -R "$Owner/$repo" -p $file -D (Split-Path $dest) --clobber 2>&1 | ForEach-Object { Say "  gh: $_" }
        $ErrorActionPreference = $ea
        if (Test-Path $dest) { return $true }
    }
    $tok = $env:GITHUB_TOKEN; if (-not $tok) { $tok = $env:GH_TOKEN }
    if ($tok) {
        Say "用 GITHUB_TOKEN 下載 $Owner/$repo $Tag/$file"
        $hdr = @{ Authorization = "Bearer $tok"; "User-Agent" = "xq-bench" }
        $rel = Invoke-RestMethod -Uri "https://api.github.com/repos/$Owner/$repo/releases/tags/$Tag" -Headers $hdr -UseBasicParsing
        $asset = @($rel.assets | Where-Object { $_.name -eq $file })[0]
        if ($asset) {
            $hdr["Accept"] = "application/octet-stream"
            Invoke-WebRequest -Uri $asset.url -Headers $hdr -OutFile $dest -UseBasicParsing
            return $true
        }
    }
    return $false
}

function Build-FromSource($bin) {
    if (-not $SrcRoot) { throw "-Build 需要 -SrcRoot（放兩個 repo 的資料夾：gpui-xq-demo、egui-xq-demo）" }
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw "-Build 需要 cargo（rustup；gpui 另需 Visual Studio Build Tools＋Windows SDK 的 fxc.exe）" }
    $jobs = @(
        @{ dir = "gpui-xq-demo"; args = "build --release -p gpui-xq-demo -p xq-feed --bins"; out = @("gpui-xq-demo.exe", "xq-tickgen.exe", "xq-demofeed.exe") },
        @{ dir = "egui-xq-demo"; args = "build --release"; out = @("egui-xq-demo.exe") }
    )
    foreach ($j in $jobs) {
        $d = Join-Path $SrcRoot $j.dir
        if (-not (Test-Path $d)) { Say "找不到 $d，跳過" "Yellow"; continue }
        Say "cargo $($j.args)（$d）"
        Push-Location $d; try { & cargo $j.args.Split(" "); if ($LASTEXITCODE) { throw "cargo 失敗：$d" } } finally { Pop-Location }
        foreach ($o in $j.out) { Copy-Item (Join-Path $d "target\release\$o") $bin -Force }
    }
}

function Resolve-Bins {
    $local = Join-Path $PSScriptRoot "bin"
    New-Item -ItemType Directory -Force $local | Out-Null
    if ($Build) { Build-FromSource $local }
    $found = @{}
    foreach ($k in $EXES.Keys) {
        $e = $EXES[$k]; $p = $null
        if ($BinDir -and (Test-Path (Join-Path $BinDir $e.file))) { $p = Join-Path $BinDir $e.file }
        elseif ((Test-Path (Join-Path $local $e.file)) -and -not $Update) { $p = Join-Path $local $e.file }
        elseif (-not $NoDownload) {
            $dest = Join-Path $local $e.file
            try { if (Download-Asset $e.repo $e.file $dest $e.private) { $p = $dest } }
            catch { Say "下載 $($e.file) 失敗：$($_.Exception.Message)" "Yellow" }
        }
        if ($p) { $found[$k] = (Resolve-Path $p).Path }
    }
    # 從網路下載／解壓的 exe 帶「網際網路來源」標記，先解除封鎖
    foreach ($p in $found.Values) { try { Unblock-File -Path $p -ErrorAction SilentlyContinue } catch { } }
    return $found
}

# ---------------------------------------------------------------- 機器資訊
function Get-MachineInfo {
    $cs = Get-CimInstance Win32_ComputerSystem
    $cpu = @(Get-CimInstance Win32_Processor)
    $gpu = @(Get-CimInstance Win32_VideoController)
    $nt = Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion"
    $build = [int]$nt.CurrentBuild
    $prod = [string]$nt.ProductName; if ($build -ge 22000) { $prod = $prod -replace "Windows 10", "Windows 11" }
    $disp = Get-P $nt "DisplayVersion"; if (-not $disp) { $disp = Get-P $nt "ReleaseId" }
    $power = $null; try { $power = ((powercfg /getactivescheme) | Out-String).Trim() } catch { }
    $bat = @(Get-CimInstance Win32_Battery -ErrorAction SilentlyContinue)
    $mons = @([XqBench]::Monitors() | ForEach-Object {
        [ordered]@{ name = $_.Name; primary = $_.Primary; x = $_.Left; y = $_.Top; width = $_.Right - $_.Left; height = $_.Bottom - $_.Top
                    dpi = $_.Dpi; scalePct = [int][math]::Round($_.Dpi / 96.0 * 100)
                    workArea = "{0},{1} {2}x{3}" -f $_.WLeft, $_.WTop, ($_.WRight - $_.WLeft), ($_.WBottom - $_.WTop) } })
    return [ordered]@{
        computer = "win"   # 不記電腦名稱（匿名化）
        os = "$prod $disp (build $build.$(Get-P $nt 'UBR'))"
        cpu = ($cpu | ForEach-Object { $_.Name.Trim() }) -join " / "
        cores = ($cpu | Measure-Object NumberOfCores -Sum).Sum
        logicalProcessors = ($cpu | Measure-Object NumberOfLogicalProcessors -Sum).Sum
        cpuMaxMHz = ($cpu | Select-Object -First 1).MaxClockSpeed
        ramGB = [math]::Round($cs.TotalPhysicalMemory / 1GB, 1)
        model = "$($cs.Manufacturer) $($cs.Model)".Trim()
        gpus = @($gpu | ForEach-Object { [ordered]@{ name = $_.Name; driver = $_.DriverVersion; driverDate = "$($_.DriverDate)"
                    mode = "$($_.CurrentHorizontalResolution)x$($_.CurrentVerticalResolution)@$($_.CurrentRefreshRate)Hz"; status = $_.Status } })
        monitors = $mons
        powerPlan = $power
        onBattery = [bool](@($bat | Where-Object { $_.BatteryStatus -eq 1 }).Count)
        dpiAwareness = $DpiMode
        powershell = "$($PSVersionTable.PSVersion) $($PSVersionTable.PSEdition)"
    }
}

# ---------------------------------------------------------------- 程式啟動／輸出解析
function New-Env($name, $extra) {
    $e = @{ XQ_BENCH = "1"; APPDATA = $Script:AppData }
    $e["XQ_PERIOD"] = "D"
    if ($name -eq "egui") { $e["XQ_RENDERER"] = "glow"; $e["XQ_EGUI_STATE"] = (Join-Path $Script:Work "egui-bench-state.json") }
    if ($name -eq "egui-wgpu") { $e["XQ_RENDERER"] = "wgpu"; $e["XQ_EGUI_STATE"] = (Join-Path $Script:Work "egui-bench-state.json") }
    if ($name -eq "gpui" -or $name -eq "gpui-quads") { $e["XQ_GPUI_STATE"] = (Join-Path $Script:Work "gpui-bench-state.json") }
    if ($name -eq "gpui") { $e["XQ_DRAW_QUADS"] = "0" }
    if ($name -eq "gpui-quads") { $e["XQ_DRAW_QUADS"] = "1" }
    foreach ($k in $extra.Keys) { $e[$k] = $extra[$k] }
    return $e
}
function Reset-State($name) {
    foreach ($f in @("egui-bench-state.json", "gpui-bench-state.json")) { Remove-Item (Join-Path $Script:Work $f) -ErrorAction SilentlyContinue }
    foreach ($d in @("gpui-xq-demo", "egui-xq-demo")) { Remove-Item (Join-Path $Script:AppData $d) -Recurse -Force -ErrorAction SilentlyContinue }
}
function Start-Proc($exe, $argline, $envs) {
    $r = New-Object XqRunner -ArgumentList $exe, $argline, (Split-Path $exe), $envs
    return @{ R = $r; Pid = $r.P.Id; Idx = 0; T0 = [XqBench]::Now(); Ev = @{}
              Fps = (New-Object System.Collections.Generic.List[object]); Tb = (New-Object System.Collections.Generic.List[object])
              Log = (New-Object System.Collections.Generic.List[string]); Gen = (New-Object System.Collections.Generic.List[object]) }
}
function Pump($st) {
    $new = $st.R.From($st.Idx); $st.Idx += $new.Length
    foreach ($x in $new) {
        $t = $x.T - $st.T0; $l = $x.L
        $i = $l.IndexOf("TICKBENCH|")
        if ($i -ge 0) { try { $st.Tb.Add(@{ T = $x.T; J = ($l.Substring($i + 10) | ConvertFrom-Json) }) } catch { $st.Log.Add(("{0,8:F3} BAD {1}" -f $t, $l)) }; continue }
        $st.Log.Add(("{0,8:F3} {1}" -f $t, $l))
        if ($x.S -eq 1 -and $l.StartsWith("{")) { try { $st.Gen.Add(@{ T = $x.T; J = ($l | ConvertFrom-Json) }) } catch { }; continue }
        foreach ($ev in @("first-frame", "first-quote", "heavy-frame")) { if ($l.Contains("BENCH $ev") -and -not $st.Ev.ContainsKey($ev)) { $st.Ev[$ev] = $t } }
        $i = $l.IndexOf("CHARTPHASE|")
        if ($i -ge 0) { $p = $l.Substring($i + 11).Split("|"); if ($p.Length -ge 2) { $st.Ev["ph:$($p[0]):$($p[1].Trim().Split(' ')[0])"] = $t } }
        foreach ($tag in @("CHARTPERF", "GROUPPERF")) {
            $i = $l.IndexOf("$tag|{")
            if ($i -ge 0) {
                try {
                    $j = $l.Substring($i + $tag.Length + 1) | ConvertFrom-Json
                    $pre = "gp"; if ($tag -eq "CHARTPERF") { $pre = "cp" }
                    $st.Ev["${pre}:$(Get-P $j 'phase')"] = @{ T = $t; J = $j }
                } catch { }
            }
        }
        if ($l.Contains("BENCH fps") -or $l.Contains("[metrics] fps=")) {
            $m = [regex]::Match($l, "fps[ =]+([0-9.]+)")
            if ($m.Success) { $st.Fps.Add(@{ T = $t; V = [double]::Parse($m.Groups[1].Value, [Globalization.CultureInfo]::InvariantCulture) }) }
        }
    }
}
function Wait-Ev($st, $key, $timeout, $name) {
    $end = [XqBench]::Now() + $timeout
    while ($true) {
        Pump $st
        if ($st.Ev.ContainsKey($key)) { return $st.Ev[$key] }
        if ($st.R.Exited) { throw "${name}: 程式結束了（exit=$($st.R.P.ExitCode)，等 $key）" }
        if ([XqBench]::Now() -gt $end) { throw "${name}: 等不到 $key" }
        Start-Sleep -Milliseconds 20
    }
}
function Wait-Gen($gen, $ev, $timeout, $name) {
    $end = [XqBench]::Now() + $timeout
    while ($true) {
        Pump $gen
        foreach ($g in $gen.Gen) {
            $e = Get-P $g.J "ev"
            if ($e -eq $ev) { return $g }
            if ($e -eq "error") { throw "xq-tickgen error: $($g.J | ConvertTo-Json -Compress)" }
        }
        if ($gen.R.Exited -and $ev -ne "end") { throw "xq-tickgen 結束了（exit=$($gen.R.P.ExitCode)，等 $ev）" }
        if ([XqBench]::Now() -gt $end) { throw "${name}: 產生器等不到 $ev" }
        Start-Sleep -Milliseconds 10
    }
}
function Stop-Proc($st) {
    # 只結束這次自己啟動的程序樹（確切 PID）
    if ($null -eq $st) { return }
    $pids = @([XqBench]::Tree($st.Pid))
    foreach ($p in ($pids | Sort-Object -Descending)) { Stop-Process -Id $p -Force -ErrorAction SilentlyContinue }
    try { [void]$st.R.P.WaitForExit(5000) } catch { }
    Start-Sleep -Milliseconds 300
    Pump $st
}
# 匿名化：結果檔不留電腦名稱／帳號／家目錄（結果會放進公開 repo）
function Anon([string]$t) {
    if (-not $t) { return $t }
    if ($env:USERPROFILE -and $env:USERPROFILE.Length -gt 3) { $t = $t.Replace($env:USERPROFILE, "C:\Users\user").Replace($env:USERPROFILE.Replace("\", "\\"), "C:\\Users\\user").Replace($env:USERPROFILE.Replace("\", "/"), "C:/Users/user") }
    if ($env:COMPUTERNAME -and $env:COMPUTERNAME.Length -gt 2) { $t = [regex]::Replace($t, [regex]::Escape($env:COMPUTERNAME), "win", "IgnoreCase") }
    if ($env:USERNAME -and $env:USERNAME.Length -gt 2) { $t = [regex]::Replace($t, "(?<![A-Za-z0-9_])" + [regex]::Escape($env:USERNAME) + "(?![A-Za-z0-9_])", "user", "IgnoreCase") }
    return $t
}
function Save-Log($st, $file, $header) {
    if ($null -eq $st) { return }
    $txt = "==== $header $(Get-Date -Format s)`r`n" + ($st.Log -join "`r`n") + "`r`n"
    [IO.File]::AppendAllText((Join-Path $Script:Out $file), (Anon $txt), [Text.Encoding]::UTF8)
}

# 記憶體：Working Set（≈ Linux RSS）、Private Bytes（commit）、Working Set - Private（≈ 工作管理員「記憶體」欄，較接近 PSS）
function Mem($root) {
    $s = [XqBench]::Snap($root)
    $wsp = [XqBench]::PrivateWsMb([XqBench]::Tree($root))
    $wp = $null; if ($wsp -ge 0) { $wp = R1 $wsp }
    return [ordered]@{ ws_mb = R1 $s.WsMb; private_mb = R1 $s.PrivateMb; wsPrivate_mb = $wp; threads = $s.Threads; nproc = $s.NProc; procs = $s.Procs }
}
function Cpu-Pct($c0, $c1, $secs) { return R1 (($c1 - $c0) / 1e7 / $secs * 100) }   # 100% = 一顆邏輯核心
function Gpu-Stop {
    $g = [XqBench]::GpuStop()
    if ($g.Samples -eq 0) { return [ordered]@{ gpu3d = $null; gpu3dMax = $null; gpuAny = $null; gpuDedicated_mb = $null; samples = 0; error = $g.Error } }
    return [ordered]@{ gpu3d = R1 $g.Gpu3dMean; gpu3dMax = R1 $g.Gpu3dMax; gpuAny = R1 $g.GpuAnyMean; gpuDedicated_mb = R1 $g.DedicatedMbMax; samples = $g.Samples; error = $g.Error }
}
function Union-Tree($root, $prev) { return [int[]]@(@([XqBench]::Tree($root)) + @($prev) | Select-Object -Unique) }

# ---------------------------------------------------------------- 視窗
function Find-Wins($st, $n, $timeout, $name) {
    $end = [XqBench]::Now() + $timeout
    while ($true) {
        Pump $st
        $ws = @([XqBench]::FindWindows([XqBench]::Tree($st.Pid), "XQ"))
        if ($ws.Count -ge $n) {
            # 主視窗（標題沒有 #k）在前，其餘照 #2、#3… 排
            return @($ws | Sort-Object { if ($_.Title -match "#(\d+)\s*$") { [int]$Matches[1] - 1 } else { 0 } } | Select-Object -First $n)
        }
        if ($st.R.Exited) { throw "${name}: 程式結束了（exit=$($st.R.P.ExitCode)，等視窗）" }
        if ([XqBench]::Now() -gt $end) { throw "${name}: 只找到 $($ws.Count)/$n 個視窗" }
        Start-Sleep -Milliseconds 20
    }
}
function Place-Main($win) {
    # 主要螢幕工作區左上角；內容區 1200×720 邏輯像素 × 該螢幕縮放
    $m = @([XqBench]::Monitors())[0]
    [XqBench]::PlaceOuter($win.H, $m.WLeft, $m.WTop, 800, 600)
    Start-Sleep -Milliseconds 400
    $scale = [XqBench]::WindowDpi($win.H) / 96.0
    $got = [XqBench]::PlaceClient($win.H, $m.WLeft, $m.WTop, [int][math]::Round($W * $scale), [int][math]::Round($H * $scale))
    Start-Sleep -Milliseconds 400
    return [ordered]@{ scale = $scale; client = $got; rect = [XqBench]::ClientRect($win.H); monitor = $m.Name }
}
function Layout-Slots($n) {
    # n ≤ 螢幕數：每個視窗佔一個螢幕的整個工作區（原生解析度／縮放）；否則輪流分配到各螢幕，同一螢幕內格狀切分
    $mons = @([XqBench]::Monitors())
    $per = @{}
    for ($k = 0; $k -lt $n; $k++) { $m = $k % $mons.Count; if (-not $per.ContainsKey($m)) { $per[$m] = @() }; $per[$m] += $k }
    $slots = @{}
    foreach ($m in $per.Keys) {
        $mon = $mons[$m]; $ks = @($per[$m]); $c = $ks.Count
        $cols = [int][math]::Ceiling([math]::Sqrt($c)); $rows = [int][math]::Ceiling($c / $cols)
        $ww = [int][math]::Floor(($mon.WRight - $mon.WLeft) / $cols); $hh = [int][math]::Floor(($mon.WBottom - $mon.WTop) / $rows)
        for ($i = 0; $i -lt $c; $i++) {
            $slots[$ks[$i]] = @{ x = $mon.WLeft + ($i % $cols) * $ww; y = $mon.WTop + [int][math]::Floor($i / $cols) * $hh; w = $ww; h = $hh; monName = $mon.Name }
        }
    }
    return $slots
}
function Place-Multi($wins) {
    $slots = Layout-Slots $wins.Count
    for ($k = 0; $k -lt $wins.Count; $k++) { $s = $slots[$k]; [XqBench]::PlaceOuter($wins[$k].H, $s.x, $s.y, $s.w, $s.h) }   # 換螢幕時 app 會收到 WM_DPICHANGED 自己縮放
    Start-Sleep -Milliseconds 700
    $out = @()
    for ($k = 0; $k -lt $wins.Count; $k++) {
        $s = $slots[$k]; [XqBench]::PlaceOuter($wins[$k].H, $s.x, $s.y, $s.w, $s.h)                                       # DPI 變更後再設一次大小
    }
    Start-Sleep -Milliseconds 500
    for ($k = 0; $k -lt $wins.Count; $k++) {
        $out += [ordered]@{ win = $k; title = $wins[$k].Title; monitor = $slots[$k].monName; dpi = [XqBench]::WindowDpi($wins[$k].H); client = [XqBench]::ClientRect($wins[$k].H) }
    }
    return $out
}
function Shot($path, $all) {
    try {
        $ms = @([XqBench]::Monitors()); if (-not $all) { $ms = @($ms[0]) }
        $l = ($ms | Measure-Object Left -Minimum).Minimum; $t = ($ms | Measure-Object Top -Minimum).Minimum
        $r = ($ms | Measure-Object Right -Maximum).Maximum; $b = ($ms | Measure-Object Bottom -Maximum).Maximum
        $bmp = New-Object System.Drawing.Bitmap -ArgumentList ([int]($r - $l)), ([int]($b - $t))
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $g.CopyFromScreen([int]$l, [int]$t, 0, 0, $bmp.Size); $g.Dispose()
        if (($r - $l) -gt 3840) {   # 多螢幕截圖縮小到 3840 寬
            $f = 3840.0 / ($r - $l)
            $sm = New-Object System.Drawing.Bitmap -ArgumentList ([int](($r - $l) * $f)), ([int](($b - $t) * $f))
            $g2 = [System.Drawing.Graphics]::FromImage($sm); $g2.DrawImage($bmp, 0, 0, $sm.Width, $sm.Height); $g2.Dispose(); $bmp.Dispose(); $bmp = $sm
        }
        $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
        return (Split-Path $path -Leaf)
    } catch { Say "截圖失敗：$($_.Exception.Message)" "Yellow"; return $null }
}

# ---------------------------------------------------------------- feed（主視窗／5 萬列／重圖表用 xq-demofeed：合成行情，約 40Hz 整批跳價，每輪重啟）
$Script:Feed = $null
function Stop-Feed { if ($Script:Feed) { Stop-Proc $Script:Feed; Save-Log $Script:Feed "demofeed.log" "feed"; $Script:Feed = $null } }
function Restart-Feed {
    if ($Script:Feed) { Stop-Feed; Start-Sleep -Milliseconds 500 }
    $Script:Feed = Start-Proc $Bins.demofeed "--addr $Addr" @{}
    if (-not (Wait-Port 30)) { throw "xq-demofeed 起不來（127.0.0.1:$Port）" }
    Start-Sleep -Milliseconds 1000   # 讓探測連線斷乾淨
}
function EvMs($st, $k) { if ($st.Ev.ContainsKey($k)) { return R1 ($st.Ev[$k] * 1000) } return $null }

# ---------------------------------------------------------------- 情境：主視窗
function Run-Main($name, $shot) {
    Restart-Feed; Reset-State $name
    $st = Start-Proc $Bins[$APP_BIN[$name]] "" (New-Env $name @{ XQ_METRICS = "1" })
    try {
        $win = @(Find-Wins $st 1 20 $name)[0]; $tWin = [XqBench]::Now() - $st.T0
        $pl = Place-Main $win
        Sleep-Until ($st.T0 + $T_IDLE); Pump $st; $idle = Mem $st.Pid
        Sleep-Until ($st.T0 + $T_CPU0)
        $p0 = [XqBench]::Tree($st.Pid); $c0 = [XqBench]::CpuSum($p0); $ta = [XqBench]::Now(); [XqBench]::GpuStart($st.Pid)
        Sleep-Until ($st.T0 + $T_END)
        $c1 = [XqBench]::CpuSum((Union-Tree $st.Pid $p0)); $tb = [XqBench]::Now()
        $gpu = Gpu-Stop
        $steady = Mem $st.Pid
        $img = $null; if ($shot) { $img = Shot (Join-Path $Script:Out "$name-main.png") $false }
        Pump $st
        $fps = @($st.Fps | Where-Object { $_.T -ge $T_CPU0 -and $_.T -le $T_END } | ForEach-Object { $_.V })
        $fa = $null; if ($fps.Count) { $fa = R1 (($fps | Measure-Object -Average).Average) }
        return [ordered]@{ app = $name; run = 0; window_ms = R1 ($tWin * 1000); first_frame_ms = (EvMs $st "first-frame"); first_quote_ms = (EvMs $st "first-quote")
            idle10 = $idle; steady60 = $steady; cpu_pct = (Cpu-Pct $c0 $c1 ($tb - $ta)); gpu = $gpu; fps = $fa; placement = $pl; screenshot = $img }
    } finally { Stop-Proc $st; Save-Log $st "$name-main.log" "main" }
}

# ---------------------------------------------------------------- 情境：5 萬列
function Run-Stress($name, $shot) {
    Restart-Feed; Reset-State $name
    $envs = New-Env $name @{ XQ_GROUP_STRESS = "1"; XQ_STRESS_COLS = "6"; XQ_STRESS_TICK_HZ = "3000"; XQ_STRESS_TICK_MS = "30000"; XQ_STRESS_SCROLL_MS = "10000" }
    $st = Start-Proc $Bins[$APP_BIN[$name]] "" $envs
    try {
        $win = @(Find-Wins $st 1 20 $name)[0]; $pl = Place-Main $win
        [void](Wait-Ev $st "gp:tick-start" 30 $name)
        $p0 = [XqBench]::Tree($st.Pid); $c0 = [XqBench]::CpuSum($p0); $ta = [XqBench]::Now(); [XqBench]::GpuStart($st.Pid)
        $img = $null
        if ($shot) { while ([XqBench]::Now() -lt $ta + 25) { Pump $st; Start-Sleep -Milliseconds 50 }; $img = Shot (Join-Path $Script:Out "$name-stress.png") $false }
        $te = Wait-Ev $st "gp:tick-end" 60 $name
        $p1 = Union-Tree $st.Pid $p0; $c1 = [XqBench]::CpuSum($p1); $tb = [XqBench]::Now()
        $gTick = Gpu-Stop; [XqBench]::GpuStart($st.Pid)
        $mTick = Mem $st.Pid
        Sleep-Until ($tb + 8); $mScroll = Mem $st.Pid
        $done = Wait-Ev $st "gp:done" 30 $name
        $c2 = [XqBench]::CpuSum((Union-Tree $st.Pid $p1)); $tc = [XqBench]::Now()
        $gScroll = Gpu-Stop
        return [ordered]@{ app = $name; run = 0; tick_end = $te.J; done = $done.J
            cpu_tick_pct = (Cpu-Pct $c0 $c1 ($tb - $ta)); cpu_scroll_pct = (Cpu-Pct $c1 $c2 ($tc - $tb))
            gpu_tick = $gTick; gpu_scroll = $gScroll; mem_tick = $mTick; mem_scroll = $mScroll; placement = $pl; screenshot = $img }
    } finally { Stop-Proc $st; Save-Log $st "$name-stress.log" "stress" }
}

# ---------------------------------------------------------------- 情境：重圖表
function Run-Heavy($name, $shot) {
    Restart-Feed; Reset-State $name
    $x = @{ XQ_CHART_STRESS = "1"; XQ_CHART_WARM_MS = "3000"; XQ_CHART_PHASE_MS = "10000"; XQ_CHART_DRAWINGS = "1000" }
    $x["XQ_TOP_FRAC"] = "0.8"
    $st = Start-Proc $Bins[$APP_BIN[$name]] "" (New-Env $name $x)
    try {
        $win = @(Find-Wins $st 1 20 $name)[0]; $pl = Place-Main $win
        $setup = Wait-Ev $st "cp:setup" 60 $name
        [void](Wait-Ev $st "heavy-frame" 30 $name)
        $out = [ordered]@{ app = $name; run = 0; heavy_frame_ms = (EvMs $st "heavy-frame"); first_quote_ms = (EvMs $st "first-quote")
            setup = $setup.J; phases = [ordered]@{}; placement = $pl; screenshot = $null }
        foreach ($ph in $HEAVY_PHASES) {
            [void](Wait-Ev $st "ph:${ph}:start" 30 $name)
            $p0 = [XqBench]::Tree($st.Pid); $c0 = [XqBench]::CpuSum($p0); $ta = [XqBench]::Now(); [XqBench]::GpuStart($st.Pid)
            $win = 5; if ($ph -eq "zshot") { $win = 1.5 }   # zshot 只有 3 秒
            while ([XqBench]::Now() -lt $ta + $win) { Pump $st; Start-Sleep -Milliseconds 50 }
            $m = Mem $st.Pid
            if ($shot -and $ph -eq "crosshair") { $out.screenshot = Shot (Join-Path $Script:Out "$name-heavy.png") $false }
            [void](Wait-Ev $st "ph:${ph}:end" 30 $name)
            $c1 = [XqBench]::CpuSum((Union-Tree $st.Pid $p0)); $tb = [XqBench]::Now()
            $g = Gpu-Stop
            $perf = Wait-Ev $st "cp:$ph" 5 $name
            $out.phases[$ph] = [ordered]@{ cpu_pct = (Cpu-Pct $c0 $c1 ($tb - $ta)); gpu = $g; mem = $m; perf = $perf.J }
        }
        # 7 段都量完才收；cp:done 緊接在 zshot 結束後
        [void](Wait-Ev $st "cp:done" 15 $name)
        return $out
    } finally { Stop-Proc $st; Save-Log $st "$name-heavy.log" "heavy" }
}

# ---------------------------------------------------------------- 情境：高頻進價（ticks）與多視窗（multi），行情＝xq-tickgen
function Counters($name, $j) {
    return @{ recv = [double](Get-P $j "rowsIn"); toUi = [double](Get-P $j "inboxRows"); applied = [double](Get-P $j "applied"); batches = [double](Get-P $j "applies") }
}
function Run-Feedgen($name, $scen, $nwin, $shot, $tag) {
    $gen = Start-Proc $Bins.tickgen "--addr $Addr $($TICK_ARGS[$scen]) --duration $DUR --settle $SETTLE --warmup $WARMUP" @{}
    $st = $null
    $logName = "$name-ticks.log"; if ($nwin -gt 0) { $logName = "$name-multi.log" }
    try {
        [void](Wait-Gen $gen "listen" 10 $name)
        Reset-State $name
        $x = @{ XQ_TICK_BENCH = "1"; XQ_FEED_ADDR = $Addr }
        if ($nwin -gt 0) { $x["XQ_WINDOWS"] = "$nwin" }
        $st = Start-Proc $Bins[$APP_BIN[$name]] "" (New-Env $name $x)
        $wins = @(Find-Wins $st ([math]::Max(1, $nwin)) 40 $name)
        if ($nwin -gt 0) { $pl = Place-Multi $wins } else { $pl = Place-Main $wins[0] }
        $gs = Wait-Gen $gen "start" 60 $name
        $tStart = $gs.T; $startUs = [double](Get-P $gs.J "unixUs")
        $p0 = [XqBench]::Tree($st.Pid); $c0 = [XqBench]::CpuSum($p0); $g0 = [XqBench]::CpuSum([int[]]@($gen.Pid)); [XqBench]::GpuStart($st.Pid)
        $mems = New-Object System.Collections.Generic.List[object]; $mems.Add(@(0.0, (Mem $st.Pid)))
        $clicks = New-Object System.Collections.Generic.List[object]
        $plan = New-Object System.Collections.Generic.List[object]
        if ($nwin -eq 0) {   # 每 6 秒點報價表 2317 列、1 秒後點 2330 列（多視窗情境不點）
            for ($k = 0; $k -lt [int][math]::Floor($DUR / 6); $k++) {
                if (5 + 6 * $k -lt $DUR - 1) { $plan.Add(@((5 + 6 * $k), "2317")) }
                if (6 + 6 * $k -lt $DUR - 1) { $plan.Add(@((6 + 6 * $k), "2330")) }
            }
        }
        $scale = [XqBench]::WindowDpi($wins[0].H) / 96.0
        $img = $null; $nextMem = 2.0
        while ($true) {
            Pump $gen; Pump $st
            $el = [XqBench]::Now() - $tStart
            if (@($gen.Gen | Where-Object { (Get-P $_.J "ev") -eq "end" }).Count -or $el -gt $DUR + 30) { break }
            if ($st.R.Exited) { throw "${name}: 程式結束了（exit=$($st.R.P.ExitCode)）" }
            if ($plan.Count -and $el -ge $plan[0][0]) {
                $sym = $plan[0][1]; $plan.RemoveAt(0)
                $xy = $CLICK[$name][$sym]
                # 起點＝按下滑鼠那一刻（Click 先移過去等 100 ms 才按，見 XqBench.cs）
                $tDown = [XqBench]::Click($wins[0].H, [int][math]::Round($xy[0] * $scale), [int][math]::Round($xy[1] * $scale))
                $clicks.Add(@($sym, [double]$tDown))
            }
            if ($el -ge $nextMem) { $s = [XqBench]::Snap($st.Pid); $mems.Add(@([math]::Round($el, 1), [ordered]@{ ws_mb = R1 $s.WsMb; private_mb = R1 $s.PrivateMb })); $nextMem += 2.0 }
            if ($shot -and -not $img -and $el -ge 15) { $img = Shot (Join-Path $Script:Out "$tag.png") ($nwin -gt 0) }
            Start-Sleep -Milliseconds 20
        }
        [XqBench]::Park()
        $ge = Wait-Gen $gen "end" 5 $name
        $tEnd = $ge.T; $secs = $tEnd - $tStart
        $c1 = [XqBench]::CpuSum((Union-Tree $st.Pid $p0)); $g1 = [XqBench]::CpuSum([int[]]@($gen.Pid))
        $gpu = Gpu-Stop
        $mEnd = Mem $st.Pid; $mems.Add(@([math]::Round($secs, 1), $mEnd))
        $tw = [XqBench]::Now() + $TAIL; while ([XqBench]::Now() -lt $tw) { Pump $st; Start-Sleep -Milliseconds 50 }
        $mTail = Mem $st.Pid
        Pump $st
        return @{ st = $st; tStart = $tStart; tEnd = $tEnd; startUs = $startUs; jEnd = $ge.J; secs = $secs
                  cpu = (Cpu-Pct $c0 $c1 $secs); genCpu = (Cpu-Pct $g0 $g1 $secs); gpu = $gpu; mems = $mems; mTail = $mTail
                  clicks = $clicks; shot = $img; placement = $pl }
    } finally {
        [void][XqBench]::GpuStop()
        if ($st) { Stop-Proc $st; Save-Log $st $logName $tag }
        Stop-Proc $gen; Save-Log $gen "tickgen.log" $tag
    }
}
function PS-Stats($v) { return [ordered]@{ p50 = R1 (Pct $v 50); p95 = R1 (Pct $v 95); p99 = R1 (Pct $v 99); max = R1 (MaxOf $v); n = $v.Count } }
function Mem-Series($mems) { return @($mems | ForEach-Object { , @($_[0], $_[1].ws_mb, $_[1].private_mb) }) }

function Run-Ticks($name, $scen, $shot) {
    $r = Run-Feedgen $name $scen 0 $shot "tick-$name-$scen"
    $tb = $r.st.Tb; $endUs = [double](Get-P $r.jEnd "unixUs"); $sent = [double](Get-P $r.jEnd "sent")
    $base = @($tb | Where-Object { $_.T -lt $r.tStart }); if (-not $base.Count) { throw "${name}/${scen}: 沒有開始前的 TICKBENCH 行" }
    $c0 = Counters $name $base[-1].J
    $after = @($tb | Where-Object { $_.T -ge $r.tEnd })
    if ($after.Count) { $cEnd = Counters $name $after[0].J } else { $cEnd = Counters $name $tb[$tb.Count - 1].J }
    $cFin = Counters $name $tb[$tb.Count - 1].J
    $L = @{}; foreach ($k in @("oldest", "newest", "transport", "toApply", "toPresent", "first5", "last5", "presents")) { $L[$k] = New-Object System.Collections.Generic.List[double] }
    foreach ($e in $tb) { foreach ($s in @(Get-P $e.J "lat")) {
        if ($null -eq $s) { continue }
        $s1 = [double]$s[1]; $s3 = [double]$s[3]; $s4 = [double]$s[4]; $s5 = [double]$s[5]; $s6 = [double]$s[6]
        if ($s1 -lt $r.startUs -or $s3 -gt $endUs) { continue }
        $o = ($s6 - $s1) / 1000; $L.oldest.Add($o); $L.newest.Add(($s6 - $s3) / 1000)
        $L.transport.Add(($s4 - $s3) / 1000); $L.toApply.Add(($s5 - $s4) / 1000); $L.toPresent.Add(($s6 - $s5) / 1000)
        if ($s1 -lt $r.startUs + 5e6) { $L.first5.Add($o) }
        if ($s1 -ge $endUs - 5e6) { $L.last5.Add($o) }
        $L.presents.Add($s6)
    } }
    $L.presents.Sort(); $maxGap = $null
    for ($i = 1; $i -lt $L.presents.Count; $i++) { $g = ($L.presents[$i] - $L.presents[$i - 1]) / 1000; if ($null -eq $maxGap -or $g -gt $maxGap) { $maxGap = $g } }
    $fps = @($tb | Where-Object { $_.T -ge $r.tStart + 1 -and $_.T -le $r.tEnd } | ForEach-Object { [double](Get-P $_.J "fps") })
    # 切換延遲：點擊時間 → app 回報的 sw [代號, 處理時間 µs, 畫完時間 µs]
    $sw = New-Object System.Collections.Generic.List[object]
    foreach ($e in $tb) { foreach ($s in @(Get-P $e.J "sw")) { if ($null -ne $s) { $sw.Add($s) } } }
    $swLat = New-Object System.Collections.Generic.List[double]; $missed = 0
    for ($k = 0; $k -lt $r.clicks.Count; $k++) {
        $sym = $r.clicks[$k][0]; $cu = [double]$r.clicks[$k][1]
        $nxt = [double]::MaxValue; if ($k + 1 -lt $r.clicks.Count) { $nxt = [double]$r.clicks[$k + 1][1] }
        $m = $null
        foreach ($s in $sw) { if ("$($s[0])" -eq $sym -and [double]$s[1] -ge $cu - 2000 -and [double]$s[1] -lt $nxt) { $m = $s; break } }
        if ($null -eq $m) { $missed++; continue }
        [void]$sw.Remove($m); $swLat.Add(([double]$m[2] - $cu) / 1000)
    }
    $l5 = R1 (Pct $L.last5 50); $f5 = R1 (Pct $L.first5 50)
    $fpsMed = $null; $fpsMin = $null; if ($fps.Count) { $fpsMed = R1 (Med $fps); $fpsMin = MinOf $fps }
    return [ordered]@{
        app = $name; run = 0; scenario = $scen; secs = R1 $r.secs
        gen = [ordered]@{ sent = $sent; target = (Get-P $r.jEnd "target"); maxDue = (Get-P $r.jEnd "maxDue"); behindFrames = (Get-P $r.jEnd "behindFrames"); maxWriteMs = (Get-P $r.jEnd "maxWriteMs"); cpu = $r.genCpu }
        latOldest = (PS-Stats $L.oldest); latNewest = (PS-Stats $L.newest)
        breakdown_p50 = [ordered]@{ transport = R1 (Pct $L.transport 50); recvToApply = R1 (Pct $L.toApply 50); applyToPresent = R1 (Pct $L.toPresent 50) }
        trend = [ordered]@{ first5_p50 = $f5; last5_p50 = $l5 }
        maxSampleGapMs = R1 $maxGap
        ticks = [ordered]@{ sent = $sent; recvFinal = $cFin.recv - $c0.recv; toUi = $cFin.toUi - $c0.toUi; applied = $cFin.applied - $c0.applied
                            batches = $cFin.batches - $c0.batches; lossFinal = $sent - ($cFin.recv - $c0.recv); backlogAtEnd = $sent - ($cEnd.recv - $c0.recv) }
        cpu = $r.cpu; gpu = $r.gpu
        mem = [ordered]@{ start = $r.mems[0][1]; end = $r.mems[$r.mems.Count - 1][1]; tail = $r.mTail; peakWs = (MaxOf @($r.mems | ForEach-Object { $_[1].ws_mb })); series = (Mem-Series $r.mems) }
        fps = $fpsMed; fpsMin = $fpsMin
        switch = [ordered]@{ p50 = R1 (Pct $swLat 50); max = R1 (MaxOf $swLat); n = $swLat.Count; missed = $missed; all = @($swLat | ForEach-Object { R1 $_ }) }
        placement = $r.placement; screenshot = $r.shot
    }
}

function Run-Multi($name, $n, $shot) {
    $r = Run-Feedgen $name "s10k" $n $shot "multi-$name-${n}w"
    $endUs = [double](Get-P $r.jEnd "unixUs"); $sent = [double](Get-P $r.jEnd "sent")
    $byWin = @{}
    foreach ($e in $r.st.Tb) {
        $k = Get-P $e.J "win"; if ($null -eq $k) { $k = 0 }; $k = [int]$k
        if (-not $byWin.ContainsKey($k)) { $byWin[$k] = New-Object System.Collections.Generic.List[object] }
        $byWin[$k].Add($e)
    }
    if (-not $byWin.ContainsKey(0)) { throw "${name}/${n}w: 沒有主視窗的 TICKBENCH" }
    $base = @($byWin[0] | Where-Object { $_.T -lt $r.tStart }); if (-not $base.Count) { throw "${name}/${n}w: 沒有開始前的 TICKBENCH 行" }
    $recv = (Counters $name $byWin[0][$byWin[0].Count - 1].J).recv - (Counters $name $base[-1].J).recv
    $per = [ordered]@{}; $pooled = New-Object System.Collections.Generic.List[double]
    for ($k = 0; $k -lt $n; $k++) {
        $lines = @(); if ($byWin.ContainsKey($k)) { $lines = $byWin[$k] }
        $old = New-Object System.Collections.Generic.List[double]
        foreach ($e in $lines) { foreach ($s in @(Get-P $e.J "lat")) {
            if ($null -eq $s) { continue }
            if ([double]$s[1] -ge $r.startUs -and [double]$s[3] -le $endUs) { $v = ([double]$s[6] - [double]$s[1]) / 1000; $old.Add($v); $pooled.Add($v) }
        } }
        $fps = @($lines | Where-Object { $_.T -ge $r.tStart + 1 -and $_.T -le $r.tEnd } | ForEach-Object { [double](Get-P $_.J "fps") })
        $fm = $null; $fn = $null; if ($fps.Count) { $fm = R1 (Med $fps); $fn = MinOf $fps }
        $per["$k"] = [ordered]@{ fps = $fm; fpsMin = $fn; p50 = R1 (Pct $old 50); p99 = R1 (Pct $old 99); max = R1 (MaxOf $old); n = $old.Count }
    }
    $fpsv = @($per.Values | ForEach-Object { $_.fps } | Where-Object { $null -ne $_ })
    $fmean = $null; if ($fpsv.Count) { $fmean = R1 (($fpsv | Measure-Object -Average).Average) }
    return [ordered]@{
        app = $name; run = 0; windows = $n; secs = R1 $r.secs
        gen = [ordered]@{ sent = $sent; cpu = $r.genCpu; maxDue = (Get-P $r.jEnd "maxDue"); behindFrames = (Get-P $r.jEnd "behindFrames") }
        ticks = [ordered]@{ sent = $sent; recv = $recv; loss = $sent - $recv }
        lat = (PS-Stats $pooled)
        perWindow = $per; fpsMin = (MinOf $fpsv); fpsMean = $fmean
        worstP99 = (MaxOf @($per.Values | ForEach-Object { $_.p99 }))
        cpu = $r.cpu; gpu = $r.gpu
        mem = [ordered]@{ start = $r.mems[0][1]; end = $r.mems[$r.mems.Count - 1][1]; tail = $r.mTail; peakWs = (MaxOf @($r.mems | ForEach-Object { $_[1].ws_mb })); series = (Mem-Series $r.mems) }
        placement = $r.placement; screenshot = $r.shot
    }
}

# ---------------------------------------------------------------- 中位數彙整
function Q($o, $path) { foreach ($k in $path.Split(".")) { $o = Get-P $o $k; if ($null -eq $o) { return $null } }; return $o }
function G($rs, $path) { return R1 (Med @($rs | ForEach-Object { Q $_ $path })) }
function Summarize($all) {
    $S = [ordered]@{}
    foreach ($name in $Apps) {
        $o = [ordered]@{}
        $rs = @($all.main | Where-Object { $_.app -eq $name })
        if ($rs.Count) {
            $o.main = [ordered]@{ runs = $rs.Count }
            foreach ($p in @(@("window_ms", "window_ms"), @("first_frame_ms", "first_frame_ms"), @("first_quote_ms", "first_quote_ms"), @("ws10_mb", "idle10.ws_mb"),
                             @("ws60_mb", "steady60.ws_mb"), @("wsPrivate60_mb", "steady60.wsPrivate_mb"), @("private60_mb", "steady60.private_mb"),
                             @("cpu_pct", "cpu_pct"), @("gpu3d", "gpu.gpu3d"), @("gpuDedicated_mb", "gpu.gpuDedicated_mb"), @("fps", "fps"),
                             @("nproc", "steady60.nproc"), @("threads", "steady60.threads"))) { $o.main[$p[0]] = G $rs $p[1] }
            $o.main.procs_last = $rs[-1].steady60.procs
        }
        $rs = @($all.stress | Where-Object { $_.app -eq $name })
        if ($rs.Count) {
            $o.stress = [ordered]@{ runs = $rs.Count }
            foreach ($p in @(@("cpu_tick_pct", "cpu_tick_pct"), @("cpu_scroll_pct", "cpu_scroll_pct"), @("gpu3d_tick", "gpu_tick.gpu3d"), @("gpu3d_scroll", "gpu_scroll.gpu3d"),
                             @("ws_tick_mb", "mem_tick.ws_mb"), @("wsPrivate_tick_mb", "mem_tick.wsPrivate_mb"), @("ws_scroll_mb", "mem_scroll.ws_mb"),
                             @("tick_fps", "tick_end.fps"), @("scroll_fps", "done.scrollFps"), @("tick_paint_ms", "tick_end.avgPaintMs"),
                             @("scroll_paint_ms", "done.avgPaintMs"), @("scroll_jank", "done.scrollJankCount"), @("rows", "done.rows"))) { $o.stress[$p[0]] = G $rs $p[1] }
        }
        $rs = @($all.heavy | Where-Object { $_.app -eq $name })
        if ($rs.Count) {
            $h = [ordered]@{ runs = $rs.Count; heavy_frame_ms = (G $rs "heavy_frame_ms")
                canvas = "$(Q $rs[-1] 'setup.cssW')x$(Q $rs[-1] 'setup.cssH')"; overlays = (Q $rs[-1] "setup.overlays"); drawings = (Q $rs[-1] "setup.drawings") }
            foreach ($ph in $HEAVY_PHASES) {
                $h[$ph] = [ordered]@{ fps = (G $rs "phases.$ph.perf.fps"); paint_ms = (G $rs "phases.$ph.perf.avgPaintMs"); cpu_pct = (G $rs "phases.$ph.cpu_pct")
                    gpu3d = (G $rs "phases.$ph.gpu.gpu3d"); ws_mb = (G $rs "phases.$ph.mem.ws_mb"); wsPrivate_mb = (G $rs "phases.$ph.mem.wsPrivate_mb") }
            }
            $o.heavy = $h
        }
        $tk = [ordered]@{}
        foreach ($sc in $Ticks) {
            $rs = @($all.ticks | Where-Object { $_.app -eq $name -and $_.scenario -eq $sc })
            if (-not $rs.Count) { continue }
            $t = [ordered]@{ runs = $rs.Count }
            foreach ($p in @(@("sent", "ticks.sent"), @("lat_p50", "latOldest.p50"), @("lat_p99", "latOldest.p99"), @("lat_max", "latOldest.max"),
                             @("lat_newest_p50", "latNewest.p50"), @("first5_p50", "trend.first5_p50"), @("last5_p50", "trend.last5_p50"),
                             @("loss", "ticks.lossFinal"), @("backlogAtEnd", "ticks.backlogAtEnd"), @("applied", "ticks.applied"), @("cpu", "cpu"), @("genCpu", "gen.cpu"),
                             @("gpu3d", "gpu.gpu3d"), @("wsStart", "mem.start.ws_mb"), @("wsEnd", "mem.end.ws_mb"), @("wsPrivateEnd", "mem.end.wsPrivate_mb"),
                             @("fps", "fps"), @("fpsMin", "fpsMin"), @("maxSampleGapMs", "maxSampleGapMs"), @("genBehindFrames", "gen.behindFrames"))) { $t[$p[0]] = G $rs $p[1] }
            $t.backlog_ms = $null; if ($null -ne $t.last5_p50 -and $null -ne $t.first5_p50) { $t.backlog_ms = R1 ($t.last5_p50 - $t.first5_p50) }
            $t.switch_p50 = R1 (Med @($rs | ForEach-Object { $_.switch.all }))
            $t.switch_n = ($rs | ForEach-Object { $_.switch.n } | Measure-Object -Sum).Sum
            $t.switch_missed = ($rs | ForEach-Object { $_.switch.missed } | Measure-Object -Sum).Sum
            $tk[$sc] = $t
        }
        if ($tk.Count) { $o.ticks = $tk }
        $mw = [ordered]@{}
        foreach ($n in $Windows) {
            $rs = @($all.multi | Where-Object { $_.app -eq $name -and $_.windows -eq $n })
            if (-not $rs.Count) { continue }
            $t = [ordered]@{ runs = $rs.Count }
            foreach ($p in @(@("loss", "ticks.loss"), @("lat_p50", "lat.p50"), @("lat_p99", "lat.p99"), @("worst_window_p99", "worstP99"), @("fps_min_window", "fpsMin"),
                             @("fps_mean", "fpsMean"), @("cpu", "cpu"), @("gpu3d", "gpu.gpu3d"), @("gpuDedicated_mb", "gpu.gpuDedicated_mb"),
                             @("wsEnd", "mem.end.ws_mb"), @("wsPrivateEnd", "mem.end.wsPrivate_mb"), @("wsPeak", "mem.peakWs"))) { $t[$p[0]] = G $rs $p[1] }
            $t.placement = $rs[-1].placement
            $mw["w$n"] = $t
        }
        if ($mw.Count) { $o.multi = $mw }
        $S[$name] = $o
    }
    return $S
}

# ---------------------------------------------------------------- summary.md（中文、結論在前、標出贏家）
$AppLabel = @{ gpui = "GPUI（DirectX 11）"; "gpui-quads" = "GPUI quad 畫線（XQ_DRAW_QUADS=1）"; egui = "egui（glow／OpenGL）"; "egui-wgpu" = "egui（wgpu）" }
function Fmt($v, $unit) { if ($null -eq $v) { return "—" } return ("{0:0.#}{1}" -f [double]$v, $unit) }
# 勝負（2026-10-24 修正）：每個 app 都和最好的比，差距在 5%（或 1 個單位：ms／%／次 1、MB 2）以內都算贏（可以多個 🏆）；
# 全部 app 都在範圍內才算平手（＝）；FPS 全部到螢幕更新率 95% 以上也算平手。舊版只有數字完全相同才平手。
function Tol($unit, $a, $b) {
    $abs = 0.0
    switch ($unit.Trim()) { "ms" { $abs = 1.0 } "%" { $abs = 1.0 } "MB" { $abs = 2.0 } "" { $abs = 1.0 } }
    return [math]::Max(0.05 * [math]::Max([math]::Abs($a), [math]::Abs($b)), $abs)
}
function Pick($S, $path, $lower, $unit = "") {
    $vals = [ordered]@{}
    foreach ($a in $Script:CurApps) { $v = Q $S[$a] $path; if ($null -ne $v) { $vals[$a] = [double]$v } }
    if ($vals.Count -lt 2) { return $null }
    $best = $null
    foreach ($a in $vals.Keys) { if ($null -eq $best -or ($lower -and $vals[$a] -lt $vals[$best]) -or (-not $lower -and $vals[$a] -gt $vals[$best])) { $best = $a } }
    $bv = $vals[$best]
    $win = @($vals.Keys | Where-Object { [math]::Abs($vals[$_] - $bv) -le (Tol $unit $vals[$_] $bv) })
    $tie = $win.Count -eq $vals.Count
    if ($path -match "fps" -and $Script:MaxHz -and (@($vals.Values | Where-Object { $_ -lt 0.95 * $Script:MaxHz }).Count -eq 0)) { $tie = $true }
    if ($tie) { $win = @() }
    return @{ best = $best; vals = $vals; tie = $tie; win = $win }
}
function Row($S, $label, $path, $unit, $lower) {
    $p = Pick $S $path $lower $unit
    $cells = @(foreach ($a in $Script:CurApps) {
        $t = Fmt (Q $S[$a] $path) $unit
        if ($p -and ($p.win -contains $a)) { "**$t** 🏆" } elseif ($p -and $p.tie -and $p.vals.Contains($a)) { "$t ＝" } else { $t }
    })
    if ($p) { foreach ($a in $p.win) { $Script:WinCount[$a]++ }; if ($p.tie) { $Script:TieCount++ } }
    return "| $label | " + ($cells -join " | ") + " |"
}
function Conclude($label, $path, $unit, $lower) {
    $p = Pick $Script:Sum $path $lower $unit; if (-not $p) { return }
    $b = $p.vals[$p.best]
    if ($p.tie) { $Script:Concl.Add("- **$label**：＝ 平手（差 5% 內）：" + ((@($p.vals.Keys) | ForEach-Object { "$_ $(Fmt $p.vals[$_] $unit)" }) -join "、")); return }
    $others = @($p.vals.Keys | Where-Object { $p.win -notcontains $_ } | ForEach-Object {
        $v = $p.vals[$_]; $ratio = ""
        if ($b -gt 0 -and $v -gt 0) { $x = $b / $v; if ($lower) { $x = $v / $b }; if ($x -ge 1.15) { $ratio = "（{0:0.#}×）" -f $x } }
        "$_ $(Fmt $v $unit)$ratio" })
    $Script:Concl.Add("- **$label**：🏆 " + ((@($p.win) | ForEach-Object { "$_ $(Fmt $p.vals[$_] $unit)" }) -join "、") + "；" + ($others -join "、"))
}
function Table($title, $rows) {
    $Script:Body.Add("### $title"); $Script:Body.Add("")
    $Script:Body.Add("| 指標 | " + (($Script:CurApps | ForEach-Object { $AppLabel[$_] }) -join " | ") + " |")
    $Script:Body.Add("|---|" + (($Script:CurApps | ForEach-Object { "---:" }) -join "|") + "|")
    foreach ($r in $rows) { $Script:Body.Add((Row $Script:Sum $r[0] $r[1] $r[2] $r[3])) }
    $Script:Body.Add("")
}
function Write-Summary($S, $mi, $path) {
    $Script:Sum = $S
    $Script:WinCount = @{}; foreach ($a in $Apps) { $Script:WinCount[$a] = 0 }; $Script:TieCount = 0
    $Script:MaxHz = $null
    foreach ($g in @($mi.gpus)) { $m = [regex]::Match("$($g.mode)", "@(\d+)Hz"); if ($m.Success -and [int]$m.Groups[1].Value -gt 0) { $Script:MaxHz = [int]$m.Groups[1].Value; break } }
    $Script:Body = New-Object System.Collections.Generic.List[string]
    $Script:Concl = New-Object System.Collections.Generic.List[string]
    if ($Scenarios -contains "main") {
        $Script:CurApps = @($Apps | Where-Object { Runs-In $_ "main" })
        Conclude "主視窗記憶體（60 秒 Working Set）" "main.ws60_mb" " MB" $true
        Conclude "主視窗待機 CPU" "main.cpu_pct" "%" $true
        Conclude "啟動到第一筆報價" "main.first_quote_ms" " ms" $true
        Table "主視窗（內容區 1200×720 邏輯像素、xq-demofeed；CPU／FPS 取第 30–60 秒）" @(
            @("視窗出現", "main.window_ms", " ms", $true), @("第一幀", "main.first_frame_ms", " ms", $true), @("第一筆報價畫出", "main.first_quote_ms", " ms", $true),
            @("Working Set 10 秒", "main.ws10_mb", " MB", $true), @("Working Set 60 秒", "main.ws60_mb", " MB", $true),
            @("Working Set - Private 60 秒", "main.wsPrivate60_mb", " MB", $true), @("Private Bytes 60 秒", "main.private60_mb", " MB", $true),
            @("CPU（100%＝1 核）", "main.cpu_pct", "%", $true), @("GPU 3D 使用率", "main.gpu3d", "%", $true), @("GPU 專用記憶體", "main.gpuDedicated_mb", " MB", $true),
            @("FPS（程式回報）", "main.fps", "", $false), @("程序數", "main.nproc", "", $true), @("執行緒數", "main.threads", "", $true))
    }
    if ($Scenarios -contains "stress") {
        $Script:CurApps = @($Apps | Where-Object { Runs-In $_ "stress" })
        Conclude "5 萬列＋每秒 3000 筆跳價 CPU" "stress.cpu_tick_pct" "%" $true
        Conclude "5 萬列捲動 FPS" "stress.scroll_fps" "" $false
        Table "5 萬列報價表（6 欄，每秒 3000 筆跳價 30 秒 → 邊跳價邊捲動 10 秒）" @(
            @("CPU 跳價段", "stress.cpu_tick_pct", "%", $true), @("CPU 捲動段", "stress.cpu_scroll_pct", "%", $true),
            @("GPU 3D 跳價段", "stress.gpu3d_tick", "%", $true), @("GPU 3D 捲動段", "stress.gpu3d_scroll", "%", $true),
            @("Working Set 跳價末", "stress.ws_tick_mb", " MB", $true), @("Working Set 捲動中", "stress.ws_scroll_mb", " MB", $true),
            @("FPS 跳價段", "stress.tick_fps", "", $false), @("FPS 捲動段", "stress.scroll_fps", "", $false),
            @("平均更新耗時 跳價段", "stress.tick_paint_ms", " ms", $true), @("平均更新耗時 捲動段", "stress.scroll_paint_ms", " ms", $true),
            @("捲動卡頓次數", "stress.scroll_jank", "", $true))
    }
    if ($Scenarios -contains "heavy") {
        $Script:CurApps = @($Apps | Where-Object { Runs-In $_ "heavy" })
        Conclude "重圖表＋十字線 FPS" "heavy.crosshair.fps" "" $false
        Conclude "重圖表＋平移 CPU" "heavy.pan.cpu_pct" "%" $true
        $rows = @(, @("到第一個重圖表幀", "heavy.heavy_frame_ms", " ms", $true))
        $pn = @{ ticks = "跳價"; crosshair = "＋十字線"; pan = "＋十字線＋平移"; drag = "拖曳畫線"; select = "框選畫線"; reorder = "調整圖層順序"; zshot = "上下順序截圖" }
        foreach ($ph in $HEAVY_PHASES) {
            $rows += , @("$($pn[$ph]) FPS", "heavy.$ph.fps", "", $false); $rows += , @("$($pn[$ph]) 繪圖耗時", "heavy.$ph.paint_ms", " ms", $true)
            $rows += , @("$($pn[$ph]) CPU", "heavy.$ph.cpu_pct", "%", $true); $rows += , @("$($pn[$ph]) GPU 3D", "heavy.$ph.gpu3d", "%", $true)
            $rows += , @("$($pn[$ph]) Working Set", "heavy.$ph.ws_mb", " MB", $true)
        }
        Table "重圖表（20 個疊加＋8 副圖＋1000 筆畫線／100 層，走勢圖佔 80% 高；7 段各 10 秒，上下順序截圖 3 秒）" $rows
    }
    if ($Scenarios -contains "ticks") {
        $Script:CurApps = @($Apps | Where-Object { Runs-In $_ "ticks" })
        $sc = "s100k"; if ($Ticks -notcontains $sc) { $sc = $Ticks[-1] }
        Conclude "高頻 $sc 延遲 p99" "ticks.$sc.lat_p99" " ms" $true
        Conclude "高頻 $sc CPU" "ticks.$sc.cpu" "%" $true
        if ($Ticks -contains "burst") { Conclude "開盤爆量延遲 p99" "ticks.burst.lat_p99" " ms" $true }
        foreach ($sc in $Ticks) {
            Table "高頻進價 $sc（xq-tickgen $($TICK_ARGS[$sc])，量 30 秒；延遲＝送出 → 畫面，取每幀最舊一包）" @(
                @("延遲 p50", "ticks.$sc.lat_p50", " ms", $true), @("延遲 p99", "ticks.$sc.lat_p99", " ms", $true), @("延遲 max", "ticks.$sc.lat_max", " ms", $true),
                @("積壓（後 5 秒 − 前 5 秒 p50）", "ticks.$sc.backlog_ms", " ms", $true), @("遺失（送出 − 收到）", "ticks.$sc.loss", "", $true),
                @("CPU（100%＝1 核）", "ticks.$sc.cpu", "%", $true), @("GPU 3D", "ticks.$sc.gpu3d", "%", $true),
                @("Working Set 結束", "ticks.$sc.wsEnd", " MB", $true), @("FPS", "ticks.$sc.fps", "", $false), @("點選切換 p50", "ticks.$sc.switch_p50", " ms", $true))
            $miss = @($Script:CurApps | ForEach-Object { Q $S[$_] "ticks.$sc.switch_missed" } | Where-Object { $_ })
            if ($miss.Count) { $Script:Body.Add("> 有點擊沒對到報價列（missed）：點選切換數字可能不準，請看截圖；可用環境變數 GPUI_ROW0_Y／GPUI_ROW1_Y 等校正。"); $Script:Body.Add("") }
        }
    }
    if ($Scenarios -contains "multi") {
        $Script:CurApps = @($Apps | Where-Object { Runs-In $_ "multi" })
        foreach ($n in $Windows) {
            Conclude "多視窗 ×$n 延遲 p99" "multi.w$n.lat_p99" " ms" $true
            Conclude "多視窗 ×$n 記憶體" "multi.w$n.wsEnd" " MB" $true
            Table "多視窗 ×$n（10k 筆/秒，視窗鋪滿螢幕、原生解析度／縮放）" @(
                @("延遲 p50（全部視窗）", "multi.w$n.lat_p50", " ms", $true), @("延遲 p99（全部視窗）", "multi.w$n.lat_p99", " ms", $true),
                @("最差視窗 p99", "multi.w$n.worst_window_p99", " ms", $true), @("最慢視窗 FPS", "multi.w$n.fps_min_window", "", $false),
                @("平均 FPS", "multi.w$n.fps_mean", "", $false), @("遺失", "multi.w$n.loss", "", $true), @("CPU", "multi.w$n.cpu", "%", $true),
                @("GPU 3D", "multi.w$n.gpu3d", "%", $true), @("GPU 專用記憶體", "multi.w$n.gpuDedicated_mb", " MB", $true),
                @("Working Set 結束", "multi.w$n.wsEnd", " MB", $true), @("Working Set - Private 結束", "multi.w$n.wsPrivateEnd", " MB", $true))
            $pl = $null; foreach ($a in $Script:CurApps) { $x = Q $S[$a] "multi.w$n.placement"; if ($x) { $pl = $x; break } }
            if ($pl) { $Script:Body.Add("視窗擺放：" + ((@($pl) | ForEach-Object { "#$($_.win + 1)→$($_.monitor)（DPI $($_.dpi)，內容區 $($_.client)）" }) -join "；")); $Script:Body.Add("") }
        }
    }
    if ($Scenarios -contains "web") { foreach ($l in @(Web-Md $all.web $Script:WebBrowser)) { $Script:Body.Add($l) } }
    $mons = (@($mi.monitors) | ForEach-Object { "$($_.width)×$($_.height) @$($_.scalePct)%$(if ($_.primary) { '（主）' })" }) -join "、"
    $gpus = (@($mi.gpus) | ForEach-Object { "$($_.name)（驅動 $($_.driver)）" }) -join "、"
    $tally = (@($Apps | Sort-Object { - $Script:WinCount[$_] }) | ForEach-Object { "$_ $($Script:WinCount[$_]) 項" }) -join "、"
    $md = New-Object System.Collections.Generic.List[string]
    $md.Add("# XQ 壓測結果（Windows）：GPUI vs egui（$($mi.computer)，$(Get-Date -Format 'yyyy-MM-dd HH:mm')）"); $md.Add("")
    $md.Add("## 結論"); $md.Add("")
    $md.Add("表格中各列最好的標 🏆；和最好的差距在 5% 以內（或 1 ms／1%／2 MB 以內）也算贏，可以多個 🏆；全部 app 都在範圍內＝平手（＝）。FPS 全部到螢幕更新率（$($Script:MaxHz) Hz）95% 以上也算平手。勝出次數：$tally；平手 $($Script:TieCount) 項。括號是落後倍數（≥ 1.15× 才標）。"); $md.Add("")
    foreach ($c in $Script:Concl) { $md.Add($c) }
    $md.Add("")
    $md.Add("- 每個數字是 $Runs 輪的中位數；CPU 100%＝一顆邏輯核心（本機 $($mi.logicalProcessors) 顆）；記憶體＝整棵程序樹。")
    $md.Add("- 延遲＝產生器送出時間 → 該筆所在畫面幀畫完（app 自己回報，是上界、最多多約 1 幀）；兩端用同一台機器的系統時鐘。")
    if (@($failures).Count) { $md.Add("- ⚠️ 有 $(@($failures).Count) 次執行失敗（已自動重跑一次），見 results.json 的 failures。") }
    $md.Add("")
    $md.Add("## 機器"); $md.Add("")
    $md.Add("| 項目 | 值 |"); $md.Add("|---|---|")
    $md.Add("| 電腦 | $($mi.computer)（$($mi.model)） |")
    $md.Add("| Windows | $($mi.os) |")
    $md.Add("| CPU | $($mi.cpu)，$($mi.cores) 核／$($mi.logicalProcessors) 執行緒 |")
    $md.Add("| RAM | $($mi.ramGB) GB |")
    $md.Add("| GPU | $gpus |")
    $md.Add("| 螢幕 | $(@($mi.monitors).Count) 個：$mons |")
    $md.Add("| 電源 | $(([string]$mi.powerPlan) -replace '\s+', ' ')$(if ($mi.onBattery) { '，⚠️ 用電池' }) |")
    $md.Add("| 執行檔 | $((@($Script:BinInfo) | ForEach-Object { "$($_.file) $($_.mb) MB" }) -join '、') |")
    $md.Add("")
    $md.Add("## 詳細數據"); $md.Add("")
    foreach ($l in $Script:Body) { $md.Add($l) }
    $md.Add("## 說明與限制"); $md.Add("")
    $md.Add("- 情境與量法同 Linux 版（gpui-xq-demo/scripts/bench-*.sh）。記憶體：Working Set（≈ Linux RSS）、Working Set - Private（≈ 工作管理員「記憶體」欄）、Private Bytes（commit）。")
    $md.Add("- GPU 使用率來自效能計數器 GPU Engine（3D 引擎、這棵程序樹、每秒取樣平均）；抓不到時顯示 —。雙顯卡筆電請確認 app 跑在哪張卡（設定 → 系統 → 顯示器 → 圖形）。")
    $md.Add("- 點選切換：腳本先把滑鼠移到報價表 2330／2317 列上（座標 × DPI 縮放），停 0.1 秒再按；延遲從按下算起。仍有 missed 時看截圖確認是否點歪。")
    $md.Add("- 測試期間不要動滑鼠鍵盤；其他程式（瀏覽器、Teams、防毒掃描）會影響 CPU 與延遲。原始數據在 results.json，各 app 輸出在 *.log。")
    [IO.File]::WriteAllText($path, (Anon (($md -join "`r`n") + "`r`n")), (New-Object Text.UTF8Encoding $true))
}

# ---------------------------------------------------------------- 網頁：一頁嵌多個元件（-Scenarios web）
# 工具包的 web\egui、web\gpui 是 egui-xq-demo／gpui-xq-demo 的 examples/web-multi 建好的網頁（wasm＋index.html＋page.js）。
# 本機開一個小網頁伺服器（XqWeb：只聽 127.0.0.1、會 gzip、收 /__bench），用全新的瀏覽器設定檔開頁面；
# 頁面自己量完（暖機 3 秒 → 5 秒幀數 → 記憶體 → 移除一格 → 讓報價表 panic），把結果 POST 回來，再關掉瀏覽器。
# 這裡另外量：頁面總下載量（gzip 後）、瀏覽器整棵程序樹的 Working Set 與 5 秒量測區間的 CPU。
$WEB_CONFIGS = [ordered]@{ mixed = ""; canvas4 = "layout=canvas4"; single = "only=table&steps=basic" }
$WEB_CONFIG_ZH = @{ mixed = "穿插（2 個元件＋2 個網頁格）"; canvas4 = "4 個元件"; single = "1 個元件（報價表）" }
$WEB_FW_ZH = [ordered]@{ egui = "egui（WebGL2）"; gpui = "GPUI（WebGPU 優先）" }
function Find-Browser {
    $c = @()
    if ($env:XQ_BROWSER) { $c += $env:XQ_BROWSER }
    foreach ($b in @($env:ProgramFiles, ${env:ProgramFiles(x86)}, $env:LOCALAPPDATA)) {
        if ($b) { $c += (Join-Path $b "Google\Chrome\Application\chrome.exe"); $c += (Join-Path $b "Microsoft\Edge\Application\msedge.exe") }
    }
    foreach ($x in $c) { if ($x -and (Test-Path $x)) { return $x } }
    return $null
}
function Web-Wait($web, [scriptblock]$pred, $timeout) {
    $end = [XqBench]::Now() + $timeout
    while ([XqBench]::Now() -lt $end) {
        foreach ($p in $web.Snapshot()) { try { $o = $p.Json | ConvertFrom-Json } catch { continue }; if (& $pred $o) { return @{ T = $p.T; O = $o } } }
        Start-Sleep -Milliseconds 100
    }
    return $null
}
function Run-Web($web, $browser, $fw, $cfg, $run) {
    $web.Reset()
    $prof = Join-Path ([IO.Path]::GetTempPath()) ("xq-web-" + [guid]::NewGuid().ToString("N").Substring(0, 8))
    $q = "bench=1"; if ($WEB_CONFIGS[$cfg]) { $q += "&" + $WEB_CONFIGS[$cfg] }
    $url = "http://127.0.0.1:$($web.Port)/$fw/index.html?$q"
    $argline = "--user-data-dir=`"$prof`" --no-first-run --no-default-browser-check --disable-extensions --disable-background-timer-throttling --disable-renderer-backgrounding --disable-backgrounding-occluded-windows --window-position=40,40 --window-size=1320,960 --new-window `"$url`""
    $t0 = [XqBench]::Now()
    $p = Start-Process -FilePath $browser -ArgumentList $argline -PassThru
    try {
        $s = Web-Wait $web { param($o) (Get-P $o "mark") -eq "fps-start" } 150
        if (-not $s) { throw "等不到頁面開始量測（150 秒）；頁面可能載入失敗" }
        $pids0 = [XqBench]::Tree($p.Id); $c0 = [XqBench]::CpuSum($pids0); $w0 = [XqBench]::Now()
        $e = Web-Wait $web { param($o) (Get-P $o "mark") -eq "fps-end" } 30
        $pids1 = Union-Tree $p.Id $pids0; $c1 = [XqBench]::CpuSum($pids1); $w1 = [XqBench]::Now()
        $m = Mem $p.Id
        $r = Web-Wait $web { param($o) ($null -ne (Get-P $o "result")) -or ($null -ne (Get-P $o "error")) } 60
        if (-not $r) { throw "等不到頁面回傳結果" }
        if ($null -ne (Get-P $r.O "error")) { throw "頁面回報錯誤：$(Get-P $r.O 'error')" }
        return [ordered]@{ framework = $fw; config = $cfg; run = $run; url = $q; userAgent = (Get-P $r.O "userAgent")
            loadToFpsStartS = R1 ($s.T - $t0); downloadBytes = $web.Sent; files = @($web.FileList() | Select-Object -First 20)
            browserProcs = $m.nproc; browserWsMb = $m.ws_mb; browserPrivateMb = $m.private_mb; browserWsPrivateMb = $m.wsPrivate_mb
            browserCpuPct = (Cpu-Pct $c0 $c1 ($w1 - $w0)); result = (Get-P $r.O "result") }
    } finally {
        foreach ($x in (@([XqBench]::Tree($p.Id)) | Sort-Object -Descending)) { Stop-Process -Id $x -Force -ErrorAction SilentlyContinue }
        Start-Sleep -Seconds 1
        Remove-Item $prof -Recurse -Force -ErrorAction SilentlyContinue
    }
}
function Web-Med($rs, [scriptblock]$get) { return R1 (Med @($rs | ForEach-Object { & $get $_ })) }
function Web-Md($recs, $browser) {
    $md = New-Object System.Collections.Generic.List[string]
    $md.Add("## 網頁：一頁嵌多個元件（同一頁放 canvas 元件和一般 HTML）"); $md.Add("")
    if (-not @($recs).Count) { $md.Add("- 沒有資料（$(if ($browser) { '全部失敗，見 failures' } else { '找不到 Chrome／Edge' })）。"); $md.Add(""); return $md }
    $ua = [string](@($recs)[0].userAgent); if ($ua.Length -gt 160) { $ua = $ua.Substring(0, 160) }
    $md.Add("- 瀏覽器：$(Split-Path $browser -Leaf)（全新設定檔、沒有擴充功能）。$ua")
    $md.Add("- egui 共用一個 wasm instance；GPUI 每個元件各自一個 instance（同一份 wasm 只下載一次）。$Runs 輪中位數。")
    $md.Add("- 「幀數」是每一格自己數的每秒畫面數（網頁格用 requestAnimationFrame）；只要 > 0 就代表那格有在動。")
    $md.Add("- 瀏覽器記憶體／CPU 是整個瀏覽器（含 GPU、分頁程序）；CPU 100%＝一顆邏輯核心。"); $md.Add("")
    $fws = @($WEB_FW_ZH.Keys | Where-Object { $f = $_; @($recs | Where-Object { $_.framework -eq $f }).Count })
    foreach ($cfg in $WEB_CONFIGS.Keys) {
        $rs = @($recs | Where-Object { $_.config -eq $cfg }); if (-not $rs.Count) { continue }
        $ids = New-Object System.Collections.Generic.List[string]
        foreach ($r in $rs) { $f = Get-P $r.result "fps"; if ($f) { foreach ($k in $f.PSObject.Properties.Name) { if (-not $ids.Contains($k)) { $ids.Add($k) } } } }
        $md.Add("### $($WEB_CONFIG_ZH[$cfg])"); $md.Add("")
        $md.Add("| 項目 | " + (($fws | ForEach-Object { $WEB_FW_ZH[$_] }) -join " | ") + " |"); $md.Add("|---|" + ("---|" * $fws.Count))
        $row = {
            param($label, [scriptblock]$get, $unit)
            $cells = foreach ($f in $fws) { $v = Web-Med @($rs | Where-Object { $_.framework -eq $f }) $get; if ($null -eq $v) { "—" } else { "$v$unit" } }
            $md.Add("| $label | $($cells -join ' | ') |")
        }
        & $row "啟動到元件都好（頁面自己量）" { param($r) Get-P $r.result "readyMs" } " ms"
        foreach ($k in $ids) { & $row "幀數：$k" ([scriptblock]::Create("param(`$r) Get-P (Get-P `$r.result 'fps') '$k'")) " fps" }
        & $row "JS heap（已用）" { param($r) Get-P (Get-P $r.result "memory") "jsHeapUsedMB" } " MB"
        & $row "wasm 記憶體（全部 instance 加總）" { param($r) Get-P (Get-P $r.result "memory") "wasmMemoryMB" } " MB"
        & $row "瀏覽器 Working Set" { param($r) $r.browserWsMb } " MB"
        & $row "瀏覽器 Working Set - Private（≈ 工作管理員）" { param($r) $r.browserWsPrivateMb } " MB"
        & $row "瀏覽器 CPU（5 秒量測區間）" { param($r) $r.browserCpuPct } " %"
        & $row "頁面總下載量（gzip）" { param($r) [double]$r.downloadBytes / 1MB } " MB"
        $removed = @($rs | ForEach-Object { Get-P $_.result "removed" } | Where-Object { $_ })
        if ($removed.Count) {
            foreach ($k in $ids) { & $row "移除 $($removed[0]) 後：$k" ([scriptblock]::Create("param(`$r) Get-P (Get-P `$r.result 'fpsAfterRemove') '$k'")) " fps" }
            foreach ($k in $ids) { & $row "報價表 panic 後：$k" ([scriptblock]::Create("param(`$r) Get-P (Get-P `$r.result 'fpsAfterPanic') '$k'")) " fps" }
        }
        $md.Add("")
    }
    $md.Add("- 判讀：移除／panic 後，被移除或 panic 的那格應該是 0 或空白，其他格要 > 0；如果其他格也變 0，就是互相拖垮。")
    $r0 = @($recs)[0].result
    $md.Add("- WebGL 繪圖器（瀏覽器回報）：$(Get-P $r0 'webglRenderer')；navigator.gpu（WebGPU）：$(if (Get-P $r0 'gpu') { '有' } else { '沒有' })。"); $md.Add("")
    return $md
}

# ================================================================ 主程式
$startedAt = Get-Date
$Script:Out = $OutDir
if (-not $Script:Out) { $Script:Out = Join-Path $PSScriptRoot ("results\{0}-win" -f (Get-Date -Format "yyyyMMdd-HHmm")) }
New-Item -ItemType Directory -Force $Script:Out | Out-Null
$Script:Out = (Resolve-Path $Script:Out).Path
$Script:Work = Join-Path ([IO.Path]::GetTempPath()) "xq-bench-work"     # 狀態檔放這裡（APPDATA 也指過來），不碰測試者自己的設定
$Script:AppData = Join-Path $Script:Work "appdata"
New-Item -ItemType Directory -Force $Script:AppData | Out-Null
$failures = New-Object System.Collections.Generic.List[string]

Say "XQ 壓測 Windows 版（GPUI vs egui）；結果放 $($Script:Out)" "Cyan"
$running = @()
foreach ($e in @("gpui-xq-demo.exe", "egui-xq-demo.exe", "xq-tickgen.exe", "xq-demofeed.exe")) { $running += @([XqBench]::FindByExe($e) | ForEach-Object { "$e（PID $_）" }) }
if ($running.Count) { throw "這些程式正在執行，請先關掉再跑（會互搶 127.0.0.1:$Port）：$($running -join '、')" }
if (Port-Busy) { throw "127.0.0.1:$Port 已被其他程式佔用，請先關掉（例如 XQ 或上次沒結束的測試程式）" }

$Bins = Resolve-Bins
$native = @($Scenarios | Where-Object { $_ -ne "web" }).Count -gt 0   # 只跑 web 時不需要原生執行檔
$missing = @(@("demofeed", "tickgen") | Where-Object { -not $Bins.ContainsKey($_) })
if ($missing.Count -and $native) { throw "缺少 $(($missing | ForEach-Object { $EXES[$_].file }) -join '、')：請放進 $PSScriptRoot\bin 或用 -BinDir 指定" }
$bad = @($Apps | Where-Object { -not $APP_BIN.ContainsKey($_) })
if ($bad.Count) { throw "不認得的 app：$($bad -join '、')（可用 $(($APP_BIN.Keys | Sort-Object) -join '、')）" }
$Apps = @($Apps | Where-Object {
    $k = $APP_BIN[$_]
    if ($Bins.ContainsKey($k)) { $true } else {
        $hint = "（下載失敗：檢查網路，或把 exe 放進 bin\）"
        Say "⚠️ 找不到 $($EXES[$k].file)，這次不量 $_$hint" "Yellow"; $false } })
if (-not $Apps.Count -and $native) { throw "一個 app 都沒有可以量" }
$Script:BinInfo = @($Bins.Keys | Sort-Object | ForEach-Object {
    $f = Get-Item $Bins[$_]
    [ordered]@{ key = $_; file = $f.Name; mb = [math]::Round($f.Length / 1MB, 1); sha256 = (Get-FileHash $f.FullName -Algorithm SHA256).Hash.Substring(0, 16); modified = $f.LastWriteTime.ToString("s") } })
foreach ($b in $Script:BinInfo) { Say ("  {0,-18} {1,6} MB  {2}" -f $b.file, $b.mb, $b.sha256) }
$buildInfo = [ordered]@{}
foreach ($t in @("gpui-build.txt", "egui-build.txt")) {
    foreach ($d in @($Bins.Values | ForEach-Object { Split-Path $_ } | Select-Object -Unique)) { $p = Join-Path $d $t; if ((Test-Path $p) -and -not $buildInfo.Contains($t)) { $buildInfo[$t] = (Get-Content $p -Raw).Trim(); Say "  $t：$($buildInfo[$t])" } }
}

$mi = Get-MachineInfo
Say "機器：$($mi.cpu)、$($mi.ramGB) GB、GPU $((@($mi.gpus) | ForEach-Object { $_.name }) -join ' / ')、螢幕 $(@($mi.monitors).Count) 個" "Cyan"
if ($mi.onBattery) { Say "⚠️ 目前用電池，請接上電源（會降頻）" "Yellow" }
if ((@($mi.monitors).Count -lt 2) -and ($Scenarios -contains "multi")) { Say "只有 1 個螢幕：多視窗 ×4 會 2×2 鋪在同一個螢幕上（有幾個就接幾個效果最好）" "Yellow" }

$est = 0; $na = @($Apps | Where-Object { $HEAVY_ONLY -notcontains $_ }).Count; $nh = $Apps.Count
if ($Scenarios -contains "main") { $est += $Runs * $na * 72 }
if ($Scenarios -contains "stress") { $est += $Runs * $na * 62 }
if ($Scenarios -contains "heavy") { $est += $Runs * $nh * 85 }
if ($Scenarios -contains "ticks") { $est += $Ticks.Count * $Runs * $na * 50 }
if ($Scenarios -contains "multi") { $est += $Windows.Count * $Runs * $na * 54 }
if ($Scenarios -contains "web") { $est += $Runs * $WEB_CONFIGS.Count * $WEB_FW_ZH.Count * 25 }
Say ("預估 {0:0} 分鐘（{1} 輪；情境 {2}；app {3}）。測試中請不要動滑鼠鍵盤，視窗會自己開關。" -f ($est / 60), $Runs, ($Scenarios -join "/"), ($Apps -join "/")) "Cyan"

$all = [ordered]@{ main = @(); stress = @(); heavy = @(); ticks = @(); multi = @(); web = @() }
$Script:WebBrowser = $null
function Try-Run($label, [scriptblock]$f) {
    for ($attempt = 1; $attempt -le 2; $attempt++) {
        try { return (& $f) }
        catch {
            $msg = "$label 第 $attempt 次失敗：$($_.Exception.Message)"
            Say "  ！$msg" "Yellow"; $failures.Add($msg)
            try { [void][XqBench]::GpuStop() } catch { }
            Start-Sleep -Seconds 3
        }
    }
    return $null
}
$t0All = Get-Date
try {
    if ($Scenarios -contains "main") {
        for ($i = 1; $i -le $Runs; $i++) { foreach ($a in $Apps) {
            if (-not (Runs-In $a "main")) { continue }
            Say "主視窗 $i/$Runs $a …"
            $r = Try-Run "main/$a" { Run-Main $a ($i -eq $Runs) }
            if ($r) { $r.run = $i; $all.main += $r; Say ("  第一筆報價 {0} ms、Working Set {1} MB、CPU {2}%、GPU {3}%、FPS {4}" -f $r.first_quote_ms, $r.steady60.ws_mb, $r.cpu_pct, $r.gpu.gpu3d, $r.fps) }
            Start-Sleep -Seconds 2 } }
    }
    if ($Scenarios -contains "stress") {
        for ($i = 1; $i -le $Runs; $i++) { foreach ($a in $Apps) {
            if (-not (Runs-In $a "stress")) { continue }
            Say "5 萬列 $i/$Runs $a …"
            $r = Try-Run "stress/$a" { Run-Stress $a ($i -eq $Runs) }
            if ($r) { $r.run = $i; $all.stress += $r; Say ("  CPU 跳價 {0}% 捲動 {1}%、Working Set {2} MB、FPS {3}/{4}" -f $r.cpu_tick_pct, $r.cpu_scroll_pct, $r.mem_tick.ws_mb, (Get-P $r.tick_end "fps"), (Get-P $r.done "scrollFps")) }
            Start-Sleep -Seconds 2 } }
    }
    if ($Scenarios -contains "heavy") {
        for ($i = 1; $i -le $Runs; $i++) { foreach ($a in $Apps) {
            Say "重圖表 $i/$Runs $a …"
            $r = Try-Run "heavy/$a" { Run-Heavy $a ($i -eq $Runs) }
            if ($r) { $r.run = $i; $all.heavy += $r; foreach ($ph in $HEAVY_PHASES) { $v = $r.phases[$ph]; Say ("  {0,-9} FPS {1}、繪圖 {2} ms、CPU {3}%、GPU {4}%" -f $ph, (Get-P $v.perf "fps"), (Get-P $v.perf "avgPaintMs"), $v.cpu_pct, $v.gpu.gpu3d) } }
            Start-Sleep -Seconds 2 } }
    }
    Stop-Feed
    if ($Scenarios -contains "ticks") {
        foreach ($sc in $Ticks) { for ($i = 1; $i -le $Runs; $i++) { foreach ($a in $Apps) {
            if (-not (Runs-In $a "ticks")) { continue }
            Say "高頻 $sc $i/$Runs $a …"
            $shot = ($i -eq $Runs) -and (($sc -eq "s50k") -or ($sc -eq "burst") -or $Quick)
            $r = Try-Run "ticks/$sc/$a" { Run-Ticks $a $sc $shot }
            if ($r) { $r.run = $i; $all.ticks += $r; Say ("  送 {0} 收 {1}、延遲 p50/p99 {2}/{3} ms、CPU {4}%、GPU {5}%、FPS {6}、切換 {7} ms（沒對到 {8}）" -f $r.ticks.sent, $r.ticks.recvFinal, $r.latOldest.p50, $r.latOldest.p99, $r.cpu, $r.gpu.gpu3d, $r.fps, $r.switch.p50, $r.switch.missed) }
            Start-Sleep -Seconds 1 } } }
    }
    if ($Scenarios -contains "multi") {
        foreach ($n in $Windows) { for ($i = 1; $i -le $Runs; $i++) { foreach ($a in $Apps) {
            if (-not (Runs-In $a "multi")) { continue }
            Say "多視窗 ×$n $i/$Runs $a …"
            $r = Try-Run "multi/${n}w/$a" { Run-Multi $a $n ($i -eq $Runs) }
            if ($r) { $r.run = $i; $all.multi += $r; Say ("  延遲 p50/p99 {0}/{1} ms、最慢視窗 FPS {2}、CPU {3}%、GPU {4}%、Working Set {5} MB" -f $r.lat.p50, $r.lat.p99, $r.fpsMin, $r.cpu, $r.gpu.gpu3d, $r.mem.end.ws_mb) }
            Start-Sleep -Seconds 1 } } }
    }
    if ($Scenarios -contains "web") {
        $webDir = $WebDir; if (-not $webDir) { $webDir = Join-Path $PSScriptRoot "web" }
        $Script:WebBrowser = Find-Browser
        $fws = @($WEB_FW_ZH.Keys | Where-Object { Test-Path (Join-Path $webDir "$_\index.html") })
        if (-not $Script:WebBrowser) { Say "⚠️ 找不到 Chrome 或 Edge，略過網頁情境" "Yellow"; $failures.Add("web：找不到 Chrome／Edge") }
        elseif (-not $fws.Count) { Say "⚠️ 工具包裡沒有網頁版（$webDir\egui、gpui），略過網頁情境" "Yellow"; $failures.Add("web：沒有 web 資料夾") }
        else {
            Say "網頁情境：$(Split-Path $Script:WebBrowser -Leaf)（會開瀏覽器視窗、跑完自動關；用暫存設定檔，不影響你平常的瀏覽器）" "Cyan"
            $web = New-Object XqWeb $webDir
            try {
                for ($i = 1; $i -le $Runs; $i++) { foreach ($cfg in @($WEB_CONFIGS.Keys)) { foreach ($fw in $fws) {
                    Say "網頁 $($WEB_CONFIG_ZH[$cfg]) $i/$Runs $fw …"
                    $r = Try-Run "web/$fw/$cfg" { Run-Web $web $Script:WebBrowser $fw $cfg $i }
                    if ($r) {
                        $all.web += $r
                        Say ("  wasm {0} MB、瀏覽器 Working Set {1} MB、CPU {2}%、下載 {3:0.00} MB、幀數 {4}" -f (Get-P (Get-P $r.result "memory") "wasmMemoryMB"), $r.browserWsMb, $r.browserCpuPct, ($r.downloadBytes / 1MB), ((Get-P $r.result "fps") | ConvertTo-Json -Compress))
                    }
                    Start-Sleep -Seconds 1 } } }
            } finally { $web.Stop() }
        }
    }
} finally {
    Stop-Feed
    try { [void][XqBench]::GpuStop() } catch { }
}

$S = Summarize $all
$res = [ordered]@{
    tool = "gpui-xq-demo/bench/windows/bench-windows.ps1"; version = 1
    started = $startedAt.ToString("o"); finished = (Get-Date).ToString("o"); minutes = [math]::Round(((Get-Date) - $t0All).TotalMinutes, 1)
    params = [ordered]@{ runs = $Runs; scenarios = $Scenarios; ticks = $Ticks; apps = $Apps; windows = $Windows; quick = [bool]$Quick }
    machine = $mi; binaries = $Script:BinInfo; builds = $buildInfo; failures = @($failures); medians = $S; raw = $all
}
[IO.File]::WriteAllText((Join-Path $Script:Out "results.json"), (Anon ($res | ConvertTo-Json -Depth 16)), (New-Object Text.UTF8Encoding $false))
Write-Summary $S $mi (Join-Path $Script:Out "summary.md")
if ($failures.Count) { Say "有 $($failures.Count) 次失敗（見 results.json 的 failures 與 *.log）" "Yellow" }
[IO.File]::WriteAllText((Join-Path $Script:Out "console.log"), (Anon ($Script:LogLines -join "`r`n")), [Text.Encoding]::UTF8)
$target = Join-Path $Script:Out "summary.md"
if (-not $NoZip) {
    $zip = "$($Script:Out).zip"
    Remove-Item $zip -ErrorAction SilentlyContinue
    Compress-Archive -Path (Join-Path $Script:Out "*") -DestinationPath $zip
    $target = $zip
    Say "完成，花了 $($res.minutes) 分鐘。請把這個檔案傳回來：$zip" "Green"
} else { Say "完成，花了 $($res.minutes) 分鐘。結果：$($Script:Out)" "Green" }
try { Start-Process explorer.exe "/select,`"$target`"" } catch { }

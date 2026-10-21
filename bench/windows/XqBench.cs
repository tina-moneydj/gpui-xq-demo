// Windows 壓測工具包的原生輔助（PowerShell 5.1 用 Add-Type 編譯；只用 C# 5 語法）。
// 程序樹／CPU／記憶體（Toolhelp＋PSAPI）、視窗尋找與擺放（user32）、螢幕與 DPI（shcore）、
// 效能計數器（PDH 英文路徑，中文 Windows 也能用）：GPU Engine 使用率、GPU 專用記憶體、Working Set - Private。
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.RegularExpressions;
using System.Threading;

public class XqLine { public double T; public int S; public string L; }

public class XqRunner {
    public Process P;
    List<XqLine> lines = new List<XqLine>();
    object lk = new object();
    public XqRunner(string exe, string args, string cwd, System.Collections.IDictionary env) {
        ProcessStartInfo si = new ProcessStartInfo(exe, args);
        si.UseShellExecute = false; si.CreateNoWindow = true;
        si.RedirectStandardError = true; si.RedirectStandardOutput = true; si.RedirectStandardInput = true;
        si.StandardErrorEncoding = Encoding.UTF8; si.StandardOutputEncoding = Encoding.UTF8;
        si.WorkingDirectory = cwd;
        if (env != null) foreach (System.Collections.DictionaryEntry e in env) si.EnvironmentVariables[e.Key.ToString()] = e.Value == null ? "" : e.Value.ToString();
        P = new Process(); P.StartInfo = si;
        P.ErrorDataReceived += (s, e) => { if (e.Data != null) Add(2, e.Data); };
        P.OutputDataReceived += (s, e) => { if (e.Data != null) Add(1, e.Data); };
        P.Start(); P.BeginErrorReadLine(); P.BeginOutputReadLine();
    }
    void Add(int s, string l) { XqLine x = new XqLine(); x.T = XqBench.Now(); x.S = s; x.L = l; lock (lk) { lines.Add(x); } }
    public XqLine[] From(int i) { lock (lk) { if (i >= lines.Count) return new XqLine[0]; return lines.GetRange(i, lines.Count - i).ToArray(); } }
    public int Count { get { lock (lk) { return lines.Count; } } }
    public bool Exited { get { try { return P.HasExited; } catch { return true; } } }
}

public class XqSnap { public double WsMb; public double PrivateMb; public int Threads; public int NProc; public string Procs; }
public class XqWin { public IntPtr H; public string Title; public int Pid; }
public class XqMon { public int Left, Top, Right, Bottom, WLeft, WTop, WRight, WBottom, Dpi; public bool Primary; public string Name; }
public class XqGpu { public double Gpu3dMean, Gpu3dMax, GpuAnyMean, GpuAnyMax, DedicatedMbMax; public int Samples; public string Error; }

public static class XqBench {
    static Stopwatch sw = Stopwatch.StartNew();
    public static double Now() { return sw.Elapsed.TotalSeconds; }
    public static long UnixUs() { return (DateTime.UtcNow.Ticks - 621355968000000000L) / 10; }

    // ---------- DPI ----------
    [DllImport("user32.dll")] static extern bool SetProcessDpiAwarenessContext(IntPtr v);
    [DllImport("shcore.dll")] static extern int SetProcessDpiAwareness(int v);
    public static string SetDpiAware() {
        try { if (SetProcessDpiAwarenessContext(new IntPtr(-4))) return "per-monitor-v2"; } catch { }
        try { if (SetProcessDpiAwareness(2) == 0) return "per-monitor"; } catch { }
        return "unaware";
    }

    // ---------- 程序樹／CPU／記憶體 ----------
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct PROCESSENTRY32W {
        public uint dwSize; public uint cntUsage; public uint th32ProcessID; public IntPtr th32DefaultHeapID; public uint th32ModuleID;
        public uint cntThreads; public uint th32ParentProcessID; public int pcPriClassBase; public uint dwFlags;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)] public string szExeFile;
    }
    [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr CreateToolhelp32Snapshot(uint flags, uint pid);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern bool Process32FirstW(IntPtr h, ref PROCESSENTRY32W e);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern bool Process32NextW(IntPtr h, ref PROCESSENTRY32W e);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint access, bool inherit, int pid);
    [DllImport("kernel32.dll")] static extern bool GetProcessTimes(IntPtr h, out long c, out long e, out long k, out long u);
    [StructLayout(LayoutKind.Sequential)]
    struct PMC_EX {
        public uint cb; public uint PageFaultCount; public UIntPtr PeakWorkingSetSize; public UIntPtr WorkingSetSize;
        public UIntPtr QuotaPeakPagedPoolUsage; public UIntPtr QuotaPagedPoolUsage; public UIntPtr QuotaPeakNonPagedPoolUsage;
        public UIntPtr QuotaNonPagedPoolUsage; public UIntPtr PagefileUsage; public UIntPtr PeakPagefileUsage; public UIntPtr PrivateUsage;
    }
    [DllImport("psapi.dll")] static extern bool GetProcessMemoryInfo(IntPtr h, ref PMC_EX c, uint cb);
    const uint QUERY = 0x1000; // PROCESS_QUERY_LIMITED_INFORMATION

    class PInfo { public int Pid, Parent, Threads; public string Exe; }
    static List<PInfo> All() {
        List<PInfo> r = new List<PInfo>();
        IntPtr s = CreateToolhelp32Snapshot(2, 0);
        if (s == new IntPtr(-1)) return r;
        PROCESSENTRY32W e = new PROCESSENTRY32W(); e.dwSize = (uint)Marshal.SizeOf(typeof(PROCESSENTRY32W));
        if (Process32FirstW(s, ref e)) {
            do { PInfo p = new PInfo(); p.Pid = (int)e.th32ProcessID; p.Parent = (int)e.th32ParentProcessID; p.Threads = (int)e.cntThreads; p.Exe = e.szExeFile; r.Add(p); }
            while (Process32NextW(s, ref e));
        }
        CloseHandle(s);
        return r;
    }
    public static int[] Tree(int root) {
        List<PInfo> all = All();
        List<int> o = new List<int>(); o.Add(root);
        for (int i = 0; i < o.Count; i++)
            foreach (PInfo p in all) if (p.Parent == o[i] && p.Pid != o[i] && !o.Contains(p.Pid)) o.Add(p.Pid);
        return o.ToArray();
    }
    public static int[] FindByExe(string exe) {
        List<int> o = new List<int>();
        foreach (PInfo p in All()) if (string.Equals(p.Exe, exe, StringComparison.OrdinalIgnoreCase)) o.Add(p.Pid);
        return o.ToArray();
    }
    static Dictionary<int, long> lastCpu = new Dictionary<int, long>();
    /// 程序樹 CPU 時間（100ns）；已結束的程序用最後一次讀到的值
    public static long CpuSum(int[] pids) {
        long sum = 0;
        foreach (int p in pids) {
            long v = -1;
            IntPtr h = OpenProcess(QUERY, false, p);
            if (h != IntPtr.Zero) { long c, e, k, u; if (GetProcessTimes(h, out c, out e, out k, out u)) v = k + u; CloseHandle(h); }
            lock (lastCpu) { if (v >= 0) lastCpu[p] = v; else if (lastCpu.ContainsKey(p)) v = lastCpu[p]; }
            if (v > 0) sum += v;
        }
        return sum;
    }
    public static XqSnap Snap(int root) {
        List<PInfo> all = All();
        int[] tree = Tree(root);
        XqSnap s = new XqSnap(); StringBuilder sb = new StringBuilder();
        foreach (int p in tree) {
            PInfo info = all.Find(delegate (PInfo x) { return x.Pid == p; });
            if (info == null) continue;
            double ws = 0, pr = 0;
            IntPtr h = OpenProcess(QUERY, false, p);
            if (h != IntPtr.Zero) {
                PMC_EX c = new PMC_EX(); c.cb = (uint)Marshal.SizeOf(typeof(PMC_EX));
                if (GetProcessMemoryInfo(h, ref c, c.cb)) { ws = (double)c.WorkingSetSize.ToUInt64() / 1048576.0; pr = (double)c.PrivateUsage.ToUInt64() / 1048576.0; }
                CloseHandle(h);
            }
            s.WsMb += ws; s.PrivateMb += pr; s.Threads += info.Threads; s.NProc++;
            sb.AppendFormat("{0}:{1}:{2:F1};", info.Exe, p, ws);
        }
        s.Procs = sb.ToString();
        return s;
    }

    // ---------- 視窗 ----------
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out int pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int hh, uint f);
    [DllImport("user32.dll")] static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] static extern uint GetDpiForWindow(IntPtr h);
    [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] static extern void mouse_event(uint f, int dx, int dy, uint d, UIntPtr e);

    public static XqWin[] FindWindows(int[] pids, string prefix) {
        List<XqWin> o = new List<XqWin>();
        HashSet<int> set = new HashSet<int>(pids);
        EnumWindows(delegate (IntPtr h, IntPtr l) {
            if (!IsWindowVisible(h)) return true;
            int pid; GetWindowThreadProcessId(h, out pid);
            if (!set.Contains(pid)) return true;
            StringBuilder sb = new StringBuilder(512); GetWindowTextW(h, sb, 512);
            string t = sb.ToString();
            if (t.StartsWith(prefix)) { XqWin w = new XqWin(); w.H = h; w.Title = t; w.Pid = pid; o.Add(w); }
            return true;
        }, IntPtr.Zero);
        return o.ToArray();
    }
    public static int WindowDpi(IntPtr h) { try { return (int)GetDpiForWindow(h); } catch { return 96; } }
    /// 視窗外框左上角放到螢幕座標 (x,y)，內容區大小 cw×ch（實體像素）；回傳實際內容區大小
    public static string PlaceClient(IntPtr h, int x, int y, int cw, int ch) {
        ShowWindow(h, 9);
        RECT wr, cr; GetWindowRect(h, out wr); GetClientRect(h, out cr);
        int dw = (wr.R - wr.L) - (cr.R - cr.L), dh = (wr.B - wr.T) - (cr.B - cr.T);
        SetWindowPos(h, IntPtr.Zero, x, y, cw + dw, ch + dh, 0x0014);
        GetClientRect(h, out cr);
        return (cr.R - cr.L) + "x" + (cr.B - cr.T);
    }
    /// 把整個視窗（含外框）放進一個矩形
    public static void PlaceOuter(IntPtr h, int x, int y, int w, int hh) { ShowWindow(h, 9); SetWindowPos(h, IntPtr.Zero, x, y, w, hh, 0x0014); }
    public static string ClientRect(IntPtr h) {
        RECT cr; GetClientRect(h, out cr); POINT o = new POINT(); ClientToScreen(h, ref o);
        return o.X + "," + o.Y + "," + (cr.R - cr.L) + "," + (cr.B - cr.T);
    }
    /// 在視窗內容區座標 (cx,cy)（實體像素）點一下左鍵
    public static void Click(IntPtr h, int cx, int cy) {
        POINT p = new POINT(); p.X = cx; p.Y = cy; ClientToScreen(h, ref p);
        SetForegroundWindow(h); SetCursorPos(p.X, p.Y);
        mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero); Thread.Sleep(15); mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);
    }
    public static void Park() { SetCursorPos(0, 0); }

    // ---------- 螢幕 ----------
    delegate bool MonProc(IntPtr m, IntPtr dc, ref RECT r, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumDisplayMonitors(IntPtr dc, IntPtr clip, MonProc cb, IntPtr l);
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct MONITORINFOEXW { public uint cbSize; public RECT rcMonitor; public RECT rcWork; public uint dwFlags; [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string szDevice; }
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern bool GetMonitorInfoW(IntPtr m, ref MONITORINFOEXW i);
    [DllImport("shcore.dll")] static extern int GetDpiForMonitor(IntPtr m, int type, out uint x, out uint y);
    public static XqMon[] Monitors() {
        List<XqMon> o = new List<XqMon>();
        EnumDisplayMonitors(IntPtr.Zero, IntPtr.Zero, delegate (IntPtr m, IntPtr dc, ref RECT r, IntPtr l) {
            MONITORINFOEXW i = new MONITORINFOEXW(); i.cbSize = (uint)Marshal.SizeOf(typeof(MONITORINFOEXW));
            GetMonitorInfoW(m, ref i);
            uint dx = 96, dy = 96; try { GetDpiForMonitor(m, 0, out dx, out dy); } catch { }
            XqMon x = new XqMon();
            x.Left = i.rcMonitor.L; x.Top = i.rcMonitor.T; x.Right = i.rcMonitor.R; x.Bottom = i.rcMonitor.B;
            x.WLeft = i.rcWork.L; x.WTop = i.rcWork.T; x.WRight = i.rcWork.R; x.WBottom = i.rcWork.B;
            x.Dpi = (int)dx; x.Primary = (i.dwFlags & 1) != 0; x.Name = i.szDevice;
            o.Add(x); return true;
        }, IntPtr.Zero);
        o.Sort(delegate (XqMon a, XqMon b) { if (a.Primary != b.Primary) return a.Primary ? -1 : 1; return a.Left != b.Left ? a.Left.CompareTo(b.Left) : a.Top.CompareTo(b.Top); });
        return o.ToArray();
    }

    // ---------- PDH（英文計數器路徑） ----------
    [DllImport("pdh.dll", CharSet = CharSet.Unicode)] static extern int PdhOpenQueryW(string src, IntPtr user, out IntPtr q);
    [DllImport("pdh.dll", CharSet = CharSet.Unicode)] static extern int PdhAddEnglishCounterW(IntPtr q, string path, IntPtr user, out IntPtr c);
    [DllImport("pdh.dll")] static extern int PdhCollectQueryData(IntPtr q);
    [DllImport("pdh.dll")] static extern int PdhCloseQuery(IntPtr q);
    [DllImport("pdh.dll", CharSet = CharSet.Unicode)] static extern int PdhGetFormattedCounterArrayW(IntPtr c, uint fmt, ref uint size, out uint count, IntPtr buf);

    static Dictionary<string, double> Read(IntPtr c) {
        Dictionary<string, double> r = new Dictionary<string, double>();
        uint size = 0, count;
        int st = PdhGetFormattedCounterArrayW(c, 0x200 | 0x8000, ref size, out count, IntPtr.Zero);
        if (size == 0) return r;
        IntPtr buf = Marshal.AllocHGlobal((int)size);
        try {
            st = PdhGetFormattedCounterArrayW(c, 0x200 | 0x8000, ref size, out count, buf);
            if (st != 0) return r;
            int ps = IntPtr.Size, item = ps == 8 ? 24 : 16, voff = ps == 8 ? 16 : 8;
            for (int i = 0; i < count; i++) {
                IntPtr it = new IntPtr(buf.ToInt64() + (long)i * item);
                string name = Marshal.PtrToStringUni(Marshal.ReadIntPtr(it));
                int cs = Marshal.ReadInt32(new IntPtr(it.ToInt64() + ps));
                if (cs != 0 && cs != 1) continue;
                double v = BitConverter.Int64BitsToDouble(Marshal.ReadInt64(new IntPtr(it.ToInt64() + voff)));
                if (name != null) r[name] = (r.ContainsKey(name) ? r[name] : 0) + v;
            }
        } finally { Marshal.FreeHGlobal(buf); }
        return r;
    }

    /// Working Set - Private（Process V2，instance＝"name:pid"，Win10 1803+）；拿不到回 -1
    public static double PrivateWsMb(int[] pids) {
        IntPtr q, c;
        if (PdhOpenQueryW(null, IntPtr.Zero, out q) != 0) return -1;
        try {
            if (PdhAddEnglishCounterW(q, "\\Process V2(*)\\Working Set - Private", IntPtr.Zero, out c) != 0) return -1;
            PdhCollectQueryData(q);
            Dictionary<string, double> d = Read(c);
            if (d.Count == 0) return -1;
            HashSet<string> want = new HashSet<string>(); foreach (int p in pids) want.Add(":" + p);
            double sum = 0;
            foreach (KeyValuePair<string, double> kv in d) { int i = kv.Key.LastIndexOf(':'); if (i >= 0 && want.Contains(kv.Key.Substring(i))) sum += kv.Value; }
            return sum / 1048576.0;
        } finally { PdhCloseQuery(q); }
    }

    // GPU 使用率取樣（背景執行緒，每秒一次；每次重算程序樹，wry 的 WebView2 GPU 程序也算進去）
    static Thread gpuThread; static volatile bool gpuRun; static XqGpu gpuRes; static int gpuRoot;
    static Regex pidRe = new Regex("pid_(\\d+)_"), engRe = new Regex("engtype_([A-Za-z0-9]+)");
    public static void GpuStart(int root) {
        GpuStop(); gpuRoot = root; gpuRes = new XqGpu(); gpuRun = true;
        gpuThread = new Thread(GpuLoop); gpuThread.IsBackground = true; gpuThread.Start();
    }
    public static XqGpu GpuStop() {
        gpuRun = false;
        if (gpuThread != null) { gpuThread.Join(5000); gpuThread = null; }
        XqGpu r = gpuRes; if (r == null) r = new XqGpu();
        if (r.Samples > 0) { r.Gpu3dMean /= r.Samples; r.GpuAnyMean /= r.Samples; }
        gpuRes = null;
        return r;
    }
    static void GpuLoop() {
        XqGpu r = gpuRes; IntPtr q, cu, cm;
        try {
            if (PdhOpenQueryW(null, IntPtr.Zero, out q) != 0) { r.Error = "PdhOpenQuery"; return; }
            int a = PdhAddEnglishCounterW(q, "\\GPU Engine(*)\\Utilization Percentage", IntPtr.Zero, out cu);
            int b = PdhAddEnglishCounterW(q, "\\GPU Process Memory(*)\\Dedicated Usage", IntPtr.Zero, out cm);
            if (a != 0) { r.Error = "GPU Engine 計數器不存在（0x" + a.ToString("X") + "）"; PdhCloseQuery(q); return; }
            PdhCollectQueryData(q);
            while (gpuRun) {
                Thread.Sleep(1000);
                if (!gpuRun) break;
                PdhCollectQueryData(q);
                HashSet<string> pids = new HashSet<string>(); foreach (int p in Tree(gpuRoot)) pids.Add(p.ToString());
                Dictionary<string, double> byEng = new Dictionary<string, double>();
                foreach (KeyValuePair<string, double> kv in Read(cu)) {
                    Match m = pidRe.Match(kv.Key); if (!m.Success || !pids.Contains(m.Groups[1].Value)) continue;
                    Match e = engRe.Match(kv.Key); string eng = e.Success ? e.Groups[1].Value : "?";
                    byEng[eng] = (byEng.ContainsKey(eng) ? byEng[eng] : 0) + kv.Value;
                }
                double d3 = byEng.ContainsKey("3D") ? Math.Min(100, byEng["3D"]) : 0, any = 0;
                foreach (double v in byEng.Values) any = Math.Max(any, Math.Min(100, v));
                r.Gpu3dMean += d3; r.GpuAnyMean += any; r.Gpu3dMax = Math.Max(r.Gpu3dMax, d3); r.GpuAnyMax = Math.Max(r.GpuAnyMax, any);
                if (b == 0) {
                    double mem = 0;
                    foreach (KeyValuePair<string, double> kv in Read(cm)) { Match m = pidRe.Match(kv.Key); if (m.Success && pids.Contains(m.Groups[1].Value)) mem += kv.Value; }
                    r.DedicatedMbMax = Math.Max(r.DedicatedMbMax, mem / 1048576.0);
                }
                r.Samples++;
            }
            PdhCloseQuery(q);
        } catch (Exception ex) { r.Error = ex.Message; }
    }
}

#!/usr/bin/env python3
"""samply 取樣檔摘要（Windows CI／Linux 通用）：每條執行緒 CPU、主執行緒各階段（自身／含子呼叫）、最熱函式、各 DLL 自身時間。

用法：python prof-summary.py profile.json.gz [--pid N | --pid-name 程式名] [--top 40] [--exe 本程式.exe --symbolizer llvm-symbolizer]
要搭配 `samply record --save-only --unstable-presymbolicate`（同目錄會有 profile.json.syms.json）。
權重用 threadCPUDelta（實際 CPU µs），沒有的話用樣本數。
"""
import bisect, gzip, json, re, sys, collections, os


def load(path):
    op = gzip.open if path.endswith(".gz") else open
    with op(path, "rt", encoding="utf-8") as f:
        p = json.load(f)
    base = path[:-3] if path.endswith(".gz") else path
    syms = None
    for cand in (base + ".syms.json", base.replace(".json", "") + ".json.syms.json"):
        if os.path.exists(cand):
            with open(cand, encoding="utf-8") as f:
                syms = json.load(f)
            break
    return p, syms


def pe_image_base(path):
    """PE 檔的 ImageBase（llvm-symbolizer 要 VA＝ImageBase＋RVA）"""
    import struct
    with open(path, "rb") as f:
        d = f.read(4096)
    off = struct.unpack_from("<I", d, 0x3C)[0]
    magic = struct.unpack_from("<H", d, off + 24)[0]
    return struct.unpack_from("<Q", d, off + 24 + 24)[0] if magic == 0x20B else struct.unpack_from("<I", d, off + 24 + 28)[0]


class ExeSymbolizer:
    """samply 沒解出來的本程式位址，改用 llvm-symbolizer（讀 PDB／DWARF）。--exe 路徑；--symbolizer 指定執行檔。"""

    def __init__(self, exe, tool, lib_names):
        self.exe, self.tool = exe, tool
        self.lib_names = {n.lower() for n in lib_names}
        self.base = pe_image_base(exe) if exe.lower().endswith(".exe") else 0
        self.cache = {}

    def wants(self, lib):
        return (lib or "").lower() in self.lib_names

    def resolve(self, addrs):
        import subprocess
        todo = sorted({a for a in addrs if a not in self.cache})
        if not todo:
            return
        inp = "\n".join(hex(self.base + a) for a in todo) + "\n"
        try:
            out = subprocess.run([self.tool, "--obj=" + self.exe, "--no-inlines", "--demangle"], input=inp,
                                 capture_output=True, text=True, encoding="utf-8", errors="replace").stdout
        except OSError as e:
            print(f"== llvm-symbolizer 跑不起來（{self.tool}）：{e}")
            return
        blocks = [b for b in out.split("\n\n")]
        for a, b in zip(todo, blocks):
            lines = [x for x in b.strip().splitlines() if x.strip()]
            name = lines[0] if lines and lines[0] != "??" else None
            self.cache[a] = name

    def lookup(self, addr):
        return self.cache.get(addr)


class Symbolizer:
    def __init__(self, syms):
        self.libs = {}
        if not syms:
            return
        st = syms["string_table"]
        for d in syms["data"]:
            raw = d.get("symbol_table", [])
            names = []
            for e in raw:
                inl = [st[fr["function"]] for fr in e.get("frames", []) or [] if fr.get("function") is not None]
                names.append((st[e["symbol"]], inl))
            known = {a: i for a, i in d.get("known_addresses", [])}
            order = sorted(range(len(raw)), key=lambda i: raw[i]["rva"])
            rvas = [raw[i]["rva"] for i in order]
            self.libs[d["debug_name"].lower()] = (rvas, order, raw, names, known)

    def lookup(self, lib, addr):
        """回傳 [最外層函式, ...內聯函式（外→內）]"""
        ent = self.libs.get((lib or "").lower())
        if not ent:
            return None
        rvas, order, raw, names, known = ent
        if addr in known:
            i = known[addr]
        else:
            j = bisect.bisect_right(rvas, addr) - 1
            if j < 0:
                return None
            i = order[j]
        sym, inl = names[i]
        if re.match(r"fun_[0-9a-f]+$", sym):
            return None  # samply 沒有真的符號，只給了位址名稱
        # 內聯函式只有短名（draw、{closure#0}），看不出是誰；預設只用外層完整符號名，XQ_PROF_INLINE=1 才展開
        if inl and os.environ.get("XQ_PROF_INLINE") == "1":
            return list(reversed(inl))
        return [sym]


def short(name):
    name = re.sub(r"<impl [^>]*>", "impl", name)
    name = re.sub(r"::h[0-9a-f]{16}$", "", name)
    name = re.sub(r"\$LT\$|\$GT\$", "", name)
    return name[:160]


# 主執行緒分桶：由外往內找第一個符合的（越後面越具體，所以取最內層符合的）
BUCKETS = [
    ("gpui: Window::draw（整幀）", r"gpui::window::Window::draw\b"),
    ("gpui: prepaint/layout（taffy）", r"taffy::|gpui::taffy::|compute_layout|request_layout"),
    ("gpui: 文字排版 layout_line", r"layout_line|LineLayoutCache|shape_line"),
    ("DirectWrite/dwrite.dll", r"^dwrite\.dll|IDWrite|DWriteCreate"),
    ("gpui: 繪圖提交 renderer.draw", r"DirectXRenderer::draw|WgpuRenderer::draw|BladeRenderer::draw|Renderer::draw"),
    ("gpui: present", r"DirectXRenderer::present|::present\b"),
    ("D3D11/DXGI/驅動", r"^(d3d11|dxgi|d3d10warp|nvwgf2umx|nvldumdx|igd10|igd11|D3DSCache|dcomp)\.dll"),
    ("我們的程式（gpui_xq_demo）", r"gpui_xq_demo::"),
]


# 主執行緒各階段（含子呼叫；同一個樣本可以同時算進好幾個階段）
PHASES = [
    ("整幀 Window::draw", r"<gpui::window::Window>::draw\b|draw_roots"),
    ("  render()（建元素樹）", r"as gpui::view::Render>::render|AnyView.*render|<gpui::view::Entity.*render"),
    ("  request_layout＋taffy 排版", r"compute_layout|<taffy::"),
    ("  prepaint", r"::prepaint$|prepaint_view"),
    ("  paint", r"Interactivity>::paint|::paint$"),
    ("  文字排版 shape_line/layout_line", r"shape_line|shape_text|layout_line|LineLayoutCache"),
    ("    DirectWrite（dwrite.dll）", r"^dwrite\.dll!|DirectWriteState>::layout_line"),
    ("  字形點陣／圖集 rasterize/atlas", r"rasterize_glyph|raster_bounds|Atlas.*get_or_insert_with"),
    ("  Scene 整理（finish/sort/clear）", r"<gpui::scene::Scene>::(finish|clear)|BoundsTree"),
    ("present／送 GPU（renderer.draw）", r"<gpui::window::Window>::present|DirectXRenderer>::draw|render_frame|WgpuRenderer.*draw"),
    ("  畫 path（中間 MSAA 圖）", r"draw_paths_to_intermediate|draw_paths_from_intermediate"),
    ("  D3D11/DXGI/驅動/WARP", r"^(d3d11|dxgi|d3d10warp|nvwgf2umx|nvldumdx|igd10\w*|igd11\w*|dcomp|D3DSCache)\.dll!"),
    ("我們的程式（gpui_xq_demo::，不含 main）", r"gpui_xq_demo::(?!main\b)\w|<gpui_xq_demo::"),
    ("  apply_feed", r"apply_feed"),
    ("  chart 畫圖", r"gpui_xq_demo::chart::"),
    ("  報價表", r"gpui_xq_demo::table::"),
    ("配置／釋放記憶體（malloc/free/HeapAlloc/mmap）", r"malloc|free\b|_free$|HeapAlloc|HeapFree|RtlAllocateHeap|RtlFreeHeap|munmap|mmap|memcpy|memmove"),
    ("訊息迴圈／等待（GetMessage/Wait/poll）", r"GetMessageW|MsgWaitForMultipleObjects|NtUserGetMessage|WaitForSingleObject|epoll|poll|futex"),
]


def main():
    args = sys.argv[1:]
    if not args:
        print(__doc__)
        return
    path = args[0]
    top = 40
    pid = None
    if "--top" in args:
        top = int(args[args.index("--top") + 1])
    if "--pid" in args:
        pid = args[args.index("--pid") + 1]
    pname = args[args.index("--pid-name") + 1] if "--pid-name" in args else None
    p, syms = load(path)
    sy = Symbolizer(syms)
    libs = p.get("libs", [])
    exe_sy = None
    if "--exe" in args:
        exe = args[args.index("--exe") + 1]
        tool = args[args.index("--symbolizer") + 1] if "--symbolizer" in args else "llvm-symbolizer"
        base = os.path.splitext(os.path.basename(exe))[0]
        exe_sy = ExeSymbolizer(exe, tool, [base + ".pdb", base.replace("-", "_") + ".pdb", os.path.basename(exe), base])
        # 先把本程式所有用到的位址一次解完
        want = []
        for t in p["threads"]:
            ft, fn, rt = t["frameTable"], t["funcTable"], t["resourceTable"]
            for fi in range(ft["length"]):
                res = fn["resource"][ft["func"][fi]]
                if res is None or res < 0:
                    continue
                li = rt["lib"][res]
                if li is None or li < 0 or li >= len(libs):
                    continue
                lname = libs[li].get("debugName") or libs[li].get("name")
                a = ft["address"][fi]
                if exe_sy.wants(lname) and a is not None and a >= 0 and not sy.lookup(lname, a):
                    want.append(a)
        exe_sy.resolve(want)
        print(f"== llvm-symbolizer：本程式位址 {len(set(want))} 個，解出 {sum(1 for v in exe_sy.cache.values() if v)} 個")
    shared_strings = p.get("shared", {}).get("stringArray")
    dur_ms = None
    meta = p.get("meta", {})

    rows = []
    for t in p["threads"]:
        if pid and str(t.get("pid")) != str(pid):
            continue
        if pname and pname.lower() not in str(t.get("processName", "")).lower():
            continue
        s = t["samples"]
        w = s.get("threadCPUDelta") or s.get("weight") or [1] * s["length"]
        w = [x or 0 for x in w]
        rows.append((sum(w), t))
    rows.sort(key=lambda r: -r[0])
    times = [x for _, t in rows for x in (t["samples"]["time"][:1] + t["samples"]["time"][-1:]) if t["samples"]["length"]]
    if times:
        dur_ms = max(times) - min(times)
    unit = "µs" if any(t["samples"].get("threadCPUDelta") for _, t in rows) else "樣本"
    total = sum(r[0] for r in rows)
    print(f"== 取樣期間約 {dur_ms/1000 if dur_ms else 0:.1f} 秒；全部執行緒 CPU 合計 {total/1000:.0f} ms（單位 {unit}）")
    if dur_ms and unit == "µs":
        print(f"== 平均佔 {total / 1000 / dur_ms * 100:.1f}% 一顆核心")
    print("\n== 各執行緒 CPU（前 20）")
    for w, t in rows[:20]:
        pct = f"{w / 1000 / dur_ms * 100:5.1f}%" if dur_ms and unit == "µs" else ""
        print(f"  {pct} {w/1000:9.0f} ms  {t.get('processName','?')}[{t.get('pid')}] {t.get('name')} tid={t.get('tid')}")

    def frames_of(t):
        strings = t.get("stringArray") or shared_strings
        ft, fn, rt = t["frameTable"], t["funcTable"], t["resourceTable"]
        cache = {}

        def frame_names(fi):
            if fi in cache:
                return cache[fi]
            func = ft["func"][fi]
            res = fn["resource"][func]
            lib = None
            if res is not None and res >= 0:
                li = rt["lib"][res]
                if li is not None and li >= 0 and li < len(libs):
                    lib = libs[li].get("debugName") or libs[li].get("name")
            addr = ft["address"][fi]
            names = None
            if addr is not None and addr >= 0 and lib:
                names = sy.lookup(lib, addr)
                if not names and exe_sy and exe_sy.wants(lib):
                    n = exe_sy.lookup(addr)
                    names = [n] if n else None
            if not names:
                nm = strings[fn["name"][func]] if fn["name"][func] is not None else "?"
                if nm.startswith("0x") and lib:
                    nm = f"{lib}!{nm}"
                names = [nm]
            names = [n if (lib is None or "!" in n) else f"{lib}!{n}" for n in names]
            cache[fi] = names
            return names

        return frame_names

    def analyze(w_total, t, label):
        s = t["samples"]
        w = s.get("threadCPUDelta") or s.get("weight") or [1] * s["length"]
        st = t["stackTable"]
        frame_names = frames_of(t)
        stack_cache = {}

        def stack_list(si):
            # 回傳由外到內的函式名稱串
            if si is None:
                return []
            if si in stack_cache:
                return stack_cache[si]
            out = []
            cur = si
            chain = []
            while cur is not None:
                chain.append(cur)
                cur = st["prefix"][cur]
            for c in reversed(chain):
                out.extend(frame_names(st["frame"][c]))
            stack_cache[si] = out
            return out

        self_c = collections.Counter()
        incl_c = collections.Counter()
        lib_c = collections.Counter()
        bucket_c = collections.Counter()
        for si, wt in zip(s["stack"], w):
            wt = wt or 0
            if not wt:
                continue
            names = stack_list(si)
            if not names:
                continue
            leaf = names[-1]
            self_c[short(leaf)] += wt
            lib = leaf.split("!", 1)[0] if "!" in leaf else "（主程式）"
            lib_c[lib] += wt
            for n in set(short(x) for x in names):
                incl_c[n] += wt
            # 分桶：取最內層符合的
            b = "其他"
            for n in names:
                for bn, rx in BUCKETS:
                    if re.search(rx, n.split("!", 1)[-1]) or re.search(rx, n):
                        b = bn
            bucket_c[b] += wt
        tot = sum(x or 0 for x in w) or 1
        phase_c = collections.Counter()
        for si, wt in zip(s["stack"], w):
            if not wt:
                continue
            names = stack_list(si)
            hit = set()
            for k, (pn, rx) in enumerate(PHASES):
                if pn in hit:
                    continue
                if any(re.search(rx, n.split("!", 1)[-1]) or re.search(rx, n) for n in names):
                    hit.add(pn)
            for pn in hit:
                phase_c[pn] += wt
        print(f"\n######## {label}：{t.get('name')} tid={t.get('tid')}，CPU {tot/1000:.0f} ms")
        print("-- 階段（含子呼叫；%＝佔這條執行緒；ms＝整段取樣期間合計）")
        for pn, _ in PHASES:
            v = phase_c.get(pn, 0)
            print(f"  {v/tot*100:5.1f}%  {v/1000:8.0f} ms  {pn}")
        print("-- 分桶（最內層命中；%＝佔這條執行緒）")
        for k, v in bucket_c.most_common():
            print(f"  {v/tot*100:5.1f}%  {k}")
        print("-- 各模組自身時間")
        for k, v in lib_c.most_common(15):
            print(f"  {v/tot*100:5.1f}%  {k}")
        print(f"-- 自身時間前 {top}")
        for k, v in self_c.most_common(top):
            print(f"  {v/tot*100:5.1f}%  {k}")
        print(f"-- 含子呼叫前 {top}（只列 gpui／我們的程式／DWrite／D3D 相關）")
        n = 0
        for k, v in incl_c.most_common(400):
            if re.search(r"gpui|xq|taffy|dwrite|IDWrite|d3d|dxgi|Direct|cosmic|wgpu|blade|swash|etagere", k, re.I):
                print(f"  {v/tot*100:5.1f}%  {k}")
                n += 1
                if n >= top:
                    break

    main_rows = [r for r in rows if r[1].get("isMainThread")] or rows[:1]
    for w, t in main_rows[:1]:
        analyze(w, t, "主執行緒")
    others = [r for r in rows if r not in main_rows[:1]]
    for w, t in others[:int(os.environ.get('XQ_PROF_OTHERS', '1'))]:
        if dur_ms and unit == "µs" and w / 1000 / dur_ms < 0.01:
            break
        analyze(w, t, "其他執行緒")


if __name__ == "__main__":
    main()

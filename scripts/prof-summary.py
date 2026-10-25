#!/usr/bin/env python3
"""samply 取樣檔摘要（Windows CI／Linux 通用）：每條執行緒 CPU、主執行緒各階段（自身／含子呼叫）、最熱函式、各 DLL 自身時間。

用法：python prof-summary.py profile.json.gz [--pid N | --pid-name 程式名] [--top 40]
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


class Symbolizer:
    def __init__(self, syms):
        self.libs = {}
        if not syms:
            return
        st = syms["string_table"]
        for d in syms["data"]:
            tab = sorted(d.get("symbol_table", []), key=lambda e: e["rva"])
            rvas = [e["rva"] for e in tab]
            names = []
            for e in tab:
                inl = [st[fr["function"]] for fr in e.get("frames", []) if "function" in fr]
                names.append((st[e["symbol"]], inl))
            self.libs[d["debug_name"].lower()] = (rvas, tab, names)

    def lookup(self, lib, addr):
        """回傳 [最外層函式, ...內聯函式（外→內）]"""
        ent = self.libs.get((lib or "").lower())
        if not ent:
            return None
        rvas, tab, names = ent
        i = bisect.bisect_right(rvas, addr) - 1
        if i < 0:
            return None
        e = tab[i]
        if addr >= e["rva"] + max(e.get("size", 0), 1) and e.get("size"):
            return None
        sym, inl = names[i]
        # frames：samply 給的是「內→外」，最後一個是最外層
        if inl:
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
        print(f"\n######## {label}：{t.get('name')} tid={t.get('tid')}，CPU {tot/1000:.0f} ms")
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
    for w, t in others[:3]:
        if dur_ms and unit == "µs" and w / 1000 / dur_ms < 0.01:
            break
        analyze(w, t, "其他執行緒")


if __name__ == "__main__":
    main()

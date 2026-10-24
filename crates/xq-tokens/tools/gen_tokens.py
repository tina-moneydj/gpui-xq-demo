#!/usr/bin/env python3
"""一次性產生 tokens/*.tokens.json（DTCG 2025.10 格式）。之後設計師直接改 JSON 或從 Figma 匯出覆蓋，不需要再跑這支。
dark = 現有程式裡寫死的顏色（逐一對照 inventory），light = 新設計的淺色主題。"""
import json, os, sys

def col(hexs, alpha=1.0, desc=None):
    h = hexs.lstrip('#').lower()
    r, g, b = (int(h[i:i+2], 16) for i in (0, 2, 4))
    v = {"colorSpace": "srgb", "components": [round(r/255, 6), round(g/255, 6), round(b/255, 6)], "alpha": alpha, "hex": "#" + h}
    t = {"$value": v}
    if desc: t["$description"] = desc
    return t

def ref(path, desc=None):
    t = {"$value": "{" + path + "}"}
    if desc: t["$description"] = desc
    return t

A22 = 0x22 / 255  # 表格漲跌閃爍底色原本是 0x22 alpha
A18 = 0x18 / 255  # 大單底色原本是 0x18 alpha

# (dark, light[, alpha, desc])
UI = {
    "bg":       ("0b0e14", "f5f6f8", 1, "視窗底色"),
    "panel":    ("10141c", "ffffff", 1, "面板底色"),
    "header":   ("171c26", "eef1f5", 1, "標題列／表頭底色"),
    "border":   ("2a3140", "d5dae1", 1, "框線"),
    "text":     ("d8dee9", "1f2430", 1, "主要文字"),
    "textDim":  ("7f8a9e", "6b7280", 1, "次要文字"),
    "accent":   ("3b82f6", "2563eb", 1, "強調色（焦點框、選取）"),
    "selected": ("1e3a5f", "dbe8fb", 1, "選取列底色"),
}
MARKET = {
    "up":         ("ff4d4f", "e0282e", 1, "漲（台股慣例紅）——報價表、標題列、成交明細"),
    "down":       ("22c55e", "0f9d48", 1, "跌（台股慣例綠）"),
    "flat":       ("e5e7eb", "4b5563", 1, "平盤"),
    "upFlash":    ("ff4d4f", "e0282e", A22, "報價跳漲時的儲存格閃爍底色"),
    "downFlash":  ("22c55e", "0f9d48", A22, "報價跳跌時的儲存格閃爍底色"),
    "volume":     ("f0c674", "a16207", 1, "成交量文字／大單文字"),
    "bigTradeBg": ("f0c674", "f59e0b", A18, "成交明細大單底色"),
}
CHART = {
    "up":             ("e5484d", "e5484d", 1, "K 棒漲"),
    "down":           ("30a46c", "1f9d5c", 1, "K 棒跌"),
    "grid":           ("242424", "e9ecf0", 1, "格線"),
    "axis":           ("8a8a8a", "6b7280", 1, "座標軸文字"),
    "text":           ("cfcfcf", "374151", 1, "圖例文字"),
    "separator":      ("333333", "d5dae1", 1, "副圖分隔線"),
    "crosshair":      ("8b95a5", "6b7280", 1, "十字線"),
    "crosshairLabel": ("3b4252", "374151", 1, "十字線座標標籤底色"),
    "midline":        ("3a3a3a", "c9ced6", 1, "副圖中線（虛線）"),
    "prevClose":      ("777777", "9ca3af", 1, "昨收虛線"),
    "areaFill":       ("5c9ded", "2563eb", 0.08, "分時走勢面積填色"),
    "tagText":        ("ffffff", "ffffff", 1, "價格標籤文字"),
    "tagBg":          ("666666", "6b7280", 1, "價格標籤底色"),
    "tooltipBg":      ("1e1e1e", "ffffff", 0.95, "游標資訊框底色"),
    "labelBgHline":   ("8a6a00", "b45309", 1, "水平線價格標籤底色"),
}
TOOLBAR = {
    "tipBg":        ("222831", "ffffff", 1, "工具列提示框底色"),
    "buttonOnBg":   ("1f3a66", "dbe8fb", 1, "工具按下底色"),
    "buttonHover":  ("2a2a2a", "eef1f5", 1, "工具滑過底色"),
    "iconOn":       ("ffffff", "1f2430", 1, "工具圖示（選中）"),
    "iconOff":      ("b8b8b8", "4b5563", 1, "工具圖示"),
    "iconDisabled": ("cfcfcf", "9ca3af", 0.3, "工具圖示（不可用）"),
    "separator":    ("333333", "d5dae1", 1, "工具列分隔線"),
}
DRAWING = {
    "stroke":       ("ffb000", "d97706", 1, "畫線中／預設畫線色"),
    "selected":     ("ffffff", "111827", 1, "選取的畫線與控制點"),
    "handleBorder": ("000000", "ffffff", 1, "控制點外框"),
    "textBg":       ("1e1e1e", "ffffff", 0.85, "文字畫線底色"),
}
OVERLAY = ["f5c542", "c678dd", "56b6c2", "ff7f50", "7fdbff", "e5484d", "30a46c", "f06292", "ffb74d", "4fc3f7",
           "aed581", "ce93d8", "80cbc4", "ffab91", "90caf9", "fff176", "ef9a9a", "b0bec5", "dce775", "b39ddb"]
OVERLAY_LIGHT = ["d4a106", "a855f7", "0891b2", "ea580c", "0284c7", "dc2626", "16a34a", "db2777", "d97706", "0369a1",
                 "65a30d", "9333ea", "0d9488", "c2410c", "2563eb", "ca8a04", "e11d48", "64748b", "84cc16", "7c3aed"]
FIB = ["9e9e9e", "e5484d", "f5a623", "4caf50", "26a69a", "4fc3f7", "9e9e9e"]
FIB_LIGHT = ["6b7280", "dc2626", "d97706", "16a34a", "0d9488", "0284c7", "6b7280"]
# 副圖指標線色（多數跟主圖疊加色同色 → 用 alias，Figma 匯出的 alias 也長這樣）
SUB = {
    "vol":  ["{color.chart.overlay.01}", "{color.chart.overlay.02}"],
    "kd":   [("f5a623", "d97706"), "{color.chart.overlay.10}"],
    "macd": ["{color.chart.overlay.01}", "{color.chart.overlay.10}"],
    "rsi":  ["{color.chart.overlay.02}"],
    "wr":   ["{color.chart.overlay.08}"],
    "dmi":  ["{color.chart.up}", "{color.chart.down}", "{color.chart.overlay.01}"],
    "atr":  ["{color.chart.overlay.09}"],
    "obv":  ["{color.chart.overlay.10}"],
}

def color_file(ix):
    def grp(d):
        return {k: col(v[ix], v[2], v[3]) for k, v in d.items()}
    chart = grp(CHART)
    chart["flat"] = ref("color.ui.accent", "分時走勢平盤時的線色（= 強調色）")
    chart["tooltipBorder"] = ref("color.ui.accent", "游標資訊框外框（= 強調色）")
    ov = OVERLAY if ix == 0 else OVERLAY_LIGHT
    chart["overlay"] = {"$description": "主圖疊加指標 1–20 的線色", **{f"{i+1:02}": col(c) for i, c in enumerate(ov)}}
    sub = {}
    for k, arr in SUB.items():
        g = {}
        for i, v in enumerate(arr):
            g[f"{i+1}"] = ref(v[1:-1]) if isinstance(v, str) else col(v[ix])
        sub[k] = g
    chart["sub"] = {"$description": "副圖指標線色", **sub}
    drawing = grp(DRAWING)
    fib = FIB if ix == 0 else FIB_LIGHT
    drawing["fib"] = {"$description": "費波納契各層線色", **{f"{i+1}": col(c) for i, c in enumerate(fib)}}
    return {
        "$description": "XQ 顏色（" + ("深色" if ix == 0 else "淺色") + "主題）。與另一個主題檔 token 名稱一一對應（Figma 一個 mode 一個檔）。",
        "color": {
            "$type": "color",
            "ui": grp(UI),
            "market": grp(MARKET),
            "chart": chart,
            "toolbar": grp(TOOLBAR),
            "drawing": drawing,
        },
    }

def dim(v, desc=None):
    t = {"$value": {"value": v, "unit": "px"}}
    if desc: t["$description"] = desc
    return t

base = {

    "$description": "XQ 字型、字級、間距、圓角、線寬、版面尺寸（深淺色共用）。",
    "font": {
        "family": {
            "$type": "fontFamily",
            "ui": {"$value": "Noto Sans CJK TC", "$description": "介面字型（GPUI 用字型名稱；egui 讀字型檔，見 README）"},
            "mono": {"$value": "Noto Sans Mono CJK TC", "$description": "數字等寬字型"},
        },
        "size": {
            "$type": "dimension",
            "xs": dim(10, "最小字（成交明細頁尾）"),
            "sm": dim(11, "座標軸、狀態列、提示"),
            "md": dim(12, "內文、報價表、按鈕"),
            "lg": dim(13, "圖表標題、區塊標題"),
            "xl": dim(15, "個股名稱"),
        },
    },
    "space": {
        "$type": "dimension",
        "xxs": dim(2), "xs": dim(4), "sm": dim(6), "md": dim(8), "lg": dim(10),
    },
    "radius": {
        "$type": "dimension",
        "xs": dim(2, "egui 版按鈕圓角（現況，與 GPUI 不同）"),
        "sm": dim(4, "GPUI 版按鈕／輸入框圓角（rounded_sm）"),
        "md": dim(6, "選單、彈出框圓角（rounded_md）"),
    },
    "stroke": {
        "$type": "dimension",
        "hairline": dim(1, "框線、分隔線"),
        "selected": dim(2, "選取中的畫線"),
        "series": dim(1.7, "egui 工具列圖示線寬"),
    },
    "layout": {
        "$type": "dimension",
        "titleBar": dim(26, "最上方標題列高"),
        "statusBar": dim(22, "最下方狀態列高"),
        "splitter": dim(5, "可拖分隔條寬"),
        "tableRow": dim(22, "報價表列高"),
        "tableHeader": dim(26, "報價表標題高"),
        "tableColHeader": dim(22, "報價表欄名列高"),
        "tapeRow": dim(20, "成交明細列高"),
        "tapeWidth": dim(276, "成交明細面板寬"),
        "tapeColHeader": dim(20, "成交明細欄名列高"),
        "tapeFooter": dim(18, "成交明細頁尾高"),
        "axisWidth": dim(66, "價格軸寬"),
        "timeAxis": dim(18, "時間軸高"),
        "chartHeader": dim(28, "圖表上方工具列高"),
        "toolRail": dim(34, "左側畫線工具列寬"),
        "toolButton": dim(28, "畫線工具按鈕邊長"),
    },
}

resolver = {
    "$schema": "https://www.designtokens.org/schemas/2025.10/resolver.json",
    "name": "XQ",
    "version": "2025.10",
    "description": "XQ 設計 token：共用 base ＋ 依 theme 選深色或淺色顏色檔。",
    "sets": {"base": {"sources": [{"$ref": "base.tokens.json"}]}},
    "modifiers": {
        "theme": {
            "description": "顏色主題",
            "contexts": {"dark": [{"$ref": "color.dark.tokens.json"}], "light": [{"$ref": "color.light.tokens.json"}]},
            "default": "dark",
        }
    },
    "resolutionOrder": [{"$ref": "#/sets/base"}, {"$ref": "#/modifiers/theme"}],
}

out = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), "..", "tokens")
def w(name, obj):
    with open(os.path.join(out, name), "w", encoding="utf-8") as f:
        json.dump(obj, f, ensure_ascii=False, indent=2); f.write("\n")
w("base.tokens.json", base)
w("color.dark.tokens.json", color_file(0))
w("color.light.tokens.json", color_file(1))
w("xq.resolver.json", resolver)
print("ok")

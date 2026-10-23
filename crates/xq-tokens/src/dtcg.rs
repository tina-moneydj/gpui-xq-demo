//! 極簡 DTCG 2025.10 解析器：只做 XQ 需要的部分（也被 build.rs 拿去在編譯時驗證 token 檔）。
//!
//! 支援：群組 `$type` 繼承、`$root`、`{a.b.c}` 別名（可串接）、整個 token 的 `$ref: "#/a/b"`、
//! color（物件：hex 優先，否則 srgb components；也接受 Tokens Studio 舊格式的 "#rrggbb[aa]" 字串）、
//! dimension（{value, unit: px|rem}；rem 以 16px 換算；也接受純數字）、fontFamily（字串或陣列取第一個）、number。
//! 不支援（XQ 用不到）：元件層級的 $ref（例如只引用 components[0]）、$extends、非 sRGB 色彩空間。

use std::collections::BTreeMap;

use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    /// 0xRRGGBB
    pub rgb: u32,
    /// 0..1
    pub a: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Color(Rgba),
    Px(f32),
    Font(String),
    Number(f64),
}

/// 一個 token 檔攤平後：路徑（a.b.c）→（$type, 原始 $value）
#[derive(Default, Clone)]
pub struct Flat(pub BTreeMap<String, (Option<String>, Value)>);

impl Flat {
    /// 把一份 DTCG 文件疊上來（同名 token 後者覆蓋前者，等同 resolver 的 resolutionOrder）
    pub fn merge_doc(&mut self, doc: &Value) -> Result<(), String> {
        walk(doc, "", None, &mut self.0)
    }

    pub fn resolve(&self) -> Result<BTreeMap<String, Tok>, Vec<String>> {
        let mut out = BTreeMap::new();
        let mut errs = Vec::new();
        for k in self.0.keys() {
            match self.resolve_one(k, 0) {
                Ok(t) => {
                    out.insert(k.clone(), t);
                }
                Err(e) => errs.push(format!("{k}: {e}")),
            }
        }
        if errs.is_empty() { Ok(out) } else { Err(errs) }
    }

    fn resolve_one(&self, path: &str, depth: u32) -> Result<Tok, String> {
        if depth > 16 {
            return Err("別名循環或太深".into());
        }
        let (ty, v) = self.0.get(path).ok_or_else(|| format!("找不到 token「{path}」"))?;
        if let Some(target) = alias_target(v) {
            let t = self.resolve_one(&target, depth + 1)?;
            return Ok(t);
        }
        let ty = ty.as_deref().ok_or("沒有 $type（自己或上層群組都沒寫）")?;
        parse_value(ty, v)
    }
}

fn walk(v: &Value, prefix: &str, inherited: Option<&str>, out: &mut BTreeMap<String, (Option<String>, Value)>) -> Result<(), String> {
    let Value::Object(m) = v else { return Err(format!("{prefix}: 應該是物件")) };
    let ty = m.get("$type").and_then(Value::as_str).or(inherited);
    if m.contains_key("$value") || m.contains_key("$ref") {
        let val = match m.get("$value") {
            Some(v) => v.clone(),
            None => Value::String(format!("$ref:{}", m["$ref"].as_str().unwrap_or_default())),
        };
        out.insert(prefix.to_string(), (ty.map(str::to_string), val));
        return Ok(());
    }
    for (k, child) in m {
        if k.starts_with('$') && k != "$root" {
            continue; // $description / $extensions / $deprecated …
        }
        if !child.is_object() {
            continue;
        }
        let p = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
        walk(child, &p, ty, out)?;
    }
    Ok(())
}

fn alias_target(v: &Value) -> Option<String> {
    let s = v.as_str()?;
    if let Some(r) = s.strip_prefix("$ref:") {
        // JSON Pointer：#/color/ui/bg 或 #/color/ui/bg/$value
        let r = r.trim_start_matches('#').trim_start_matches('/');
        let r = r.strip_suffix("/$value").unwrap_or(r);
        return Some(r.replace('/', "."));
    }
    let s = s.trim();
    (s.starts_with('{') && s.ends_with('}')).then(|| s[1..s.len() - 1].to_string())
}

fn parse_value(ty: &str, v: &Value) -> Result<Tok, String> {
    match ty {
        "color" => parse_color(v).map(Tok::Color),
        "dimension" => match v {
            Value::Number(n) => Ok(Tok::Px(n.as_f64().unwrap_or(0.0) as f32)),
            Value::Object(m) => {
                let n = m.get("value").and_then(Value::as_f64).ok_or("dimension 缺 value")?;
                match m.get("unit").and_then(Value::as_str).unwrap_or("px") {
                    "px" => Ok(Tok::Px(n as f32)),
                    "rem" => Ok(Tok::Px((n * 16.0) as f32)),
                    u => Err(format!("不支援的單位 {u}（請用 px 或 rem）")),
                }
            }
            _ => Err("dimension 格式錯".into()),
        },
        "fontFamily" => match v {
            Value::String(s) => Ok(Tok::Font(s.clone())),
            Value::Array(a) => a.first().and_then(Value::as_str).map(|s| Tok::Font(s.to_string())).ok_or_else(|| "fontFamily 陣列是空的".into()),
            _ => Err("fontFamily 格式錯".into()),
        },
        "number" => v.as_f64().map(Tok::Number).ok_or_else(|| "number 格式錯".into()),
        other => Err(format!("XQ 還沒用到的 $type「{other}」")),
    }
}

fn parse_hex(s: &str) -> Option<(u32, Option<f32>)> {
    let h = s.trim().strip_prefix('#')?;
    let full: String = match h.len() {
        3 | 4 => h.chars().flat_map(|c| [c, c]).collect(),
        6 | 8 => h.to_string(),
        _ => return None,
    };
    let n = u32::from_str_radix(&full, 16).ok()?;
    Some(if full.len() == 8 { (n >> 8, Some((n & 0xff) as f32 / 255.0)) } else { (n, None) })
}

fn parse_color(v: &Value) -> Result<Rgba, String> {
    match v {
        Value::String(s) => {
            let (rgb, a) = parse_hex(s).ok_or_else(|| format!("看不懂的顏色「{s}」"))?;
            Ok(Rgba { rgb, a: a.unwrap_or(1.0) })
        }
        Value::Object(m) => {
            let alpha = m.get("alpha").and_then(Value::as_f64).map(|a| a as f32);
            if let Some((rgb, a8)) = m.get("hex").and_then(Value::as_str).and_then(parse_hex) {
                return Ok(Rgba { rgb, a: alpha.or(a8).unwrap_or(1.0) });
            }
            let space = m.get("colorSpace").and_then(Value::as_str).unwrap_or("srgb");
            if space != "srgb" {
                return Err(format!("沒有 hex 時只支援 srgb（收到 {space}）"));
            }
            let c = m.get("components").and_then(Value::as_array).ok_or("color 缺 components")?;
            if c.len() != 3 {
                return Err("components 要 3 個".into());
            }
            let mut rgb = 0u32;
            for x in c {
                let f = x.as_f64().ok_or("components 要是數字（不支援 none）")?;
                rgb = (rgb << 8) | (f.clamp(0.0, 1.0) * 255.0).round() as u32;
            }
            Ok(Rgba { rgb, a: alpha.unwrap_or(1.0) })
        }
        _ => Err("color 格式錯".into()),
    }
}

/// 照 xq.resolver.json 的 resolutionOrder 把檔案疊起來；`theme` 選 modifiers.theme 的 context。
/// `read(name)` 回傳檔案內容（內嵌或從磁碟讀）。
pub fn resolve_theme(read: &dyn Fn(&str) -> Result<String, String>, theme: &str) -> Result<BTreeMap<String, Tok>, Vec<String>> {
    let one = |e: String| vec![e];
    let parse = |name: &str| -> Result<Value, Vec<String>> {
        let s = read(name).map_err(one)?;
        serde_json::from_str(&s).map_err(|e| vec![format!("{name}: JSON 格式錯：{e}")])
    };
    let res = parse("xq.resolver.json")?;
    let mut flat = Flat::default();
    let load_sources = |srcs: &Value, flat: &mut Flat| -> Result<(), Vec<String>> {
        for s in srcs.as_array().into_iter().flatten() {
            if let Some(file) = s.get("$ref").and_then(Value::as_str) {
                let doc = parse(file)?;
                flat.merge_doc(&doc).map_err(|e| vec![format!("{file}: {e}")])?;
            } else {
                flat.merge_doc(s).map_err(one)?; // inline tokens
            }
        }
        Ok(())
    };
    for item in res.get("resolutionOrder").and_then(Value::as_array).ok_or_else(|| one("resolver 缺 resolutionOrder".into()))? {
        let r = item.get("$ref").and_then(Value::as_str).unwrap_or_default();
        if let Some(set) = r.strip_prefix("#/sets/") {
            let srcs = &res["sets"][set]["sources"];
            load_sources(srcs, &mut flat)?;
        } else if let Some(m) = r.strip_prefix("#/modifiers/") {
            let md = &res["modifiers"][m];
            let ctx = if m == "theme" { theme.to_string() } else { md["default"].as_str().unwrap_or_default().to_string() };
            let srcs = &md["contexts"][ctx.as_str()];
            if srcs.is_null() {
                return Err(one(format!("modifier {m} 沒有 context「{ctx}」")));
            }
            load_sources(srcs, &mut flat)?;
        } else {
            return Err(one(format!("resolutionOrder 看不懂：{r}")));
        }
    }
    flat.resolve()
}

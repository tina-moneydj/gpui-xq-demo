//! 編譯時驗證 tokens/：JSON 格式、別名、每個主題都有程式要的每個 token。設計師改錯會直接編譯失敗並列出哪個 token。
#[allow(dead_code)]
#[path = "src/dtcg.rs"]
mod dtcg;
#[allow(dead_code)]
#[path = "src/theme.rs"]
mod theme;

fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tokens");
    println!("cargo:rerun-if-changed={}", dir.display());
    for f in ["xq.resolver.json", "base.tokens.json", "color.dark.tokens.json", "color.light.tokens.json"] {
        println!("cargo:rerun-if-changed={}", dir.join(f).display());
    }
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).map_err(|e| format!("{name}: {e}"));
    let mut bad = Vec::new();
    for mode in ["dark", "light"] {
        match dtcg::resolve_theme(&read, mode).and_then(|m| theme::Theme::from_tokens("x", &m)) {
            Ok(_) => {}
            Err(es) => bad.extend(es.into_iter().map(|e| format!("[{mode}] {e}"))),
        }
    }
    if !bad.is_empty() {
        panic!("\n設計 token 檔有問題（xq-tokens/tokens）：\n  {}\n", bad.join("\n  "));
    }
}

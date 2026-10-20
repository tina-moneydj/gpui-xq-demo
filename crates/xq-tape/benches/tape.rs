//! micro-bench 範例（不用外部套件，`harness = false` 自己量）。
//!
//! 【照這個寫】每個熱路徑函式都配一個 bench，並且**寫上預算**（budget）。
//! 超過預算就印 `OVER`，CI 可以 `XQ_BENCH_STRICT=1 cargo bench -p xq-tape` 讓它直接失敗。
//!
//! 預算從哪來：高頻壓測（每秒 10 萬筆報價）時，背景執行緒每筆報價都會呼叫 `on_quote`；
//! 10 萬 × 50 ns = 5 ms／秒 = 0.5% 一顆核心，可以接受。畫面每幀只格式化可見列（約 20 列），
//! 20 × 1 µs = 20 µs，遠低於 16 ms 的幀預算。

use std::hint::black_box;
use std::time::Instant;

use xq_feed::Quote;
use xq_tape::{format_row, Ring, Side, TapeCore, Trade};

struct Case {
    name: &'static str,
    ns_per_op: f64,
    budget_ns: f64,
}

fn measure(name: &'static str, budget_ns: f64, iters: u64, mut f: impl FnMut(u64)) -> Case {
    // 暖身：讓 CPU 頻率、快取、分支預測穩定
    for i in 0..iters / 10 {
        f(i);
    }
    let t0 = Instant::now();
    for i in 0..iters {
        f(i);
    }
    Case { name, ns_per_op: t0.elapsed().as_nanos() as f64 / iters as f64, budget_ns }
}

fn main() {
    // 預先建好報價（量測不含建 String 的成本）；90% 是別檔，10% 是目前代號——接近 50k 列壓測的比例
    let quotes: Vec<Quote> = (0..4096)
        .map(|i| Quote {
            symbol: if i % 10 == 0 { "2330".into() } else { format!("Z{i:05}") },
            name: String::new(),
            price: 1000.0 + (i % 7) as f64,
            change: 1.0,
            volume: i as i64,
        })
        .collect();

    let mut cases = Vec::new();

    let mut core = TapeCore::new("2330");
    cases.push(measure("TapeCore::on_quote（每筆報價，背景執行緒）", 50.0, 5_000_000, |i| {
        let q = &quotes[(i % 4096) as usize];
        black_box(core.on_quote(black_box(q), i));
    }));

    let mut ring = Ring::with_capacity(5000);
    let t = Trade { seq: 0, ts_ms: 0, price: 1000.0, change: 1.0, qty: 3, side: Side::Up };
    cases.push(measure("Ring::push（每筆成交）", 10.0, 10_000_000, |i| {
        ring.push(black_box(Trade { seq: i, ..t }));
    }));

    cases.push(measure("Ring::newest（每個可見列）", 10.0, 10_000_000, |i| {
        black_box(ring.newest(black_box((i % 5000) as usize)));
    }));

    cases.push(measure("format_row（每個可見列，render 時）", 1000.0, 1_000_000, |i| {
        black_box(format_row(black_box(&Trade { seq: i, ..t }), 28_800));
    }));

    let strict = std::env::var_os("XQ_BENCH_STRICT").is_some();
    let mut over = 0;
    println!("{:<44} {:>12} {:>12}  結果", "項目", "ns/次", "預算 ns");
    for c in &cases {
        let ok = c.ns_per_op <= c.budget_ns;
        if !ok {
            over += 1;
        }
        println!("{:<44} {:>12.1} {:>12.0}  {}", c.name, c.ns_per_op, c.budget_ns, if ok { "OK" } else { "OVER" });
    }
    if strict && over > 0 {
        eprintln!("{over} 項超過預算");
        std::process::exit(1);
    }
}

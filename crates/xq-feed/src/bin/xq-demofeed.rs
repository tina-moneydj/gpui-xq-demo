//! xq-demofeed：`WryFeedHost --demo-ticks` 的 Rust 移植版（同一個協議、同樣的節奏），不需要 .NET。
//!
//! 給 macOS 測試包用：主視窗／5 萬列／重圖表三個情境原本在 Linux／Windows 由 WryFeedHost --demo-ticks 供應行情，
//! Mac 上改用這支，兩邊 app 收到的封包型態一樣。
//!
//! 行為對照 wry-xq-demo/feedhost/WryFeedHost/DemoTicks.cs＋TcpServer.cs：
//! - 127.0.0.1:47631（`--addr` 可改），一次服務一個 client（新連線進來就換人，同 WryFeedHost「latest wins」）。
//! - 連上先送 hello(ready=1)。收到訂閱：記下代號（最多 64）與走勢圖代號；有走勢圖代號就送一次 120 根分時（type 3）。
//!   空訂閱＝退訂、停送。
//! - 每 25ms（約 40Hz）每檔訂閱代號跳一次價（±0.15% 隨機走、量加 10–400），整批一包 type 2（WryFeedHost 每 25ms flush 一次、
//!   同代號只留最新，結果相同）；每 40 次（約 1 秒）送走勢圖代號一根分鐘 K（type 4）。
//! - 只是路徑測試用的合成行情，**不是真實行情**。
//!
//! 用法：xq-demofeed [--addr 127.0.0.1:47631] [--interval-ms 25]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const MAX_SYMBOLS: usize = 64;

const NAMES: &[(&str, &str)] = &[
    ("2330", "台積電"), ("2317", "鴻海"), ("2454", "聯發科"), ("2303", "聯電"), ("2881", "富邦金"),
    ("2882", "國泰金"), ("AAPL", "Apple"), ("TSLA", "Tesla"), ("NVDA", "NVIDIA"),
];

#[derive(Default)]
struct Sub {
    gen: u64,
    symbols: Vec<String>,
    chart: Option<String>,
}

struct St {
    prev: f64,
    price: f64,
    volume: i64,
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.unit() * (hi - lo) as f64) as i64
    }
}

fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

fn state_for(s: &str) -> St {
    let seed = fnv(s);
    let prev = 50.0 + (seed % 900) as f64;
    St { prev, price: prev, volume: (seed % 100_000) as i64 }
}

fn name_of(s: &str) -> &str {
    NAMES.iter().find(|(k, _)| *k == s).map(|(_, n)| *n).unwrap_or(s)
}

fn cents(px: f64) -> i32 {
    (px * 100.0).round() as i32
}

fn text(body: &mut Vec<u8>, t: &str) {
    let mut n = t.len().min(96);
    while !t.is_char_boundary(n) {
        n -= 1;
    }
    body.push(n as u8);
    body.extend_from_slice(&t.as_bytes()[..n]);
}

fn frame(body: Vec<u8>) -> Vec<u8> {
    let mut f = Vec::with_capacity(body.len() + 4);
    f.extend_from_slice(&(body.len() as u32).to_le_bytes());
    f.extend_from_slice(&body);
    f
}

fn bar(body: &mut Vec<u8>, t: u16, o: i32, h: i32, l: i32, c: i32, v: i64) {
    body.extend_from_slice(&t.to_le_bytes());
    for x in [o, h, l, c] {
        body.extend_from_slice(&x.to_le_bytes());
    }
    body.extend_from_slice(&v.to_le_bytes());
}

fn minute_of_day() -> u16 {
    // 本機時間的當日分鐘（libc 不在依賴裡；用 UTC＋TZ 偏移環境變數 XQ_TZ_MIN，預設 +480 台北）
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let off: i64 = std::env::var("XQ_TZ_MIN").ok().and_then(|v| v.parse().ok()).unwrap_or(480);
    (((secs / 60 + off).rem_euclid(1440)) as u16).min(1439)
}

fn parse_sub(p: &[u8]) -> Option<(bool, String, Vec<String>)> {
    if p.first() != Some(&1) {
        return None;
    }
    let mut i = 1;
    let chart_on = *p.get(i)? != 0;
    i += 1;
    let rd = |i: &mut usize| -> Option<String> {
        let n = *p.get(*i)? as usize;
        let s = p.get(*i + 1..*i + 1 + n)?;
        *i += 1 + n;
        Some(String::from_utf8_lossy(s).into_owned())
    };
    let chart = rd(&mut i)?;
    let n = u16::from_le_bytes([*p.get(i)?, *p.get(i + 1)?]) as usize;
    i += 2;
    let mut syms = Vec::new();
    for _ in 0..n.min(MAX_SYMBOLS) {
        let s = rd(&mut i)?;
        let s = s.trim().to_string();
        if !s.is_empty() && !syms.contains(&s) {
            syms.push(s);
        }
    }
    Some((chart_on, chart, syms))
}

fn intraday(sym: &str, st: &St) -> Vec<u8> {
    let mut rng = Rng(fnv(sym) | 1);
    let mut body = vec![3u8];
    text(&mut body, sym);
    body.extend_from_slice(&cents(st.prev).to_le_bytes());
    body.extend_from_slice(&120u16.to_le_bytes());
    let start = minute_of_day().saturating_sub(120);
    let mut px = st.prev;
    for i in 0..120u16 {
        let o = px + (rng.unit() - 0.5) * 2.0;
        let c = o + (rng.unit() - 0.5) * 2.0;
        let h = o.max(c) + rng.unit();
        let l = o.min(c) - rng.unit();
        bar(&mut body, start + i, cents(o), cents(h), cents(l), cents(c), rng.range(50, 3000));
        px = c;
    }
    frame(body)
}

fn serve(stream: TcpStream, my_gen: u64, current: Arc<AtomicU64>, interval: Duration) -> std::io::Result<()> {
    stream.set_nodelay(true)?;
    let mut w = stream.try_clone()?;
    let sub = Arc::new(Mutex::new(Sub::default()));
    let closed = Arc::new(AtomicBool::new(false));
    {
        let (sub, closed) = (sub.clone(), closed.clone());
        let mut r = stream;
        thread::spawn(move || {
            let mut buf = vec![0u8; 16 * 1024];
            let mut acc: Vec<u8> = Vec::new();
            loop {
                match r.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        acc.extend_from_slice(&buf[..n]);
                        while acc.len() >= 4 {
                            let len = u32::from_le_bytes([acc[0], acc[1], acc[2], acc[3]]) as usize;
                            if len == 0 || len > 8_000_000 {
                                closed.store(true, Ordering::Relaxed);
                                return;
                            }
                            if acc.len() < 4 + len {
                                break;
                            }
                            if let Some((chart_on, chart, syms)) = parse_sub(&acc[4..4 + len]) {
                                println!("[xq-demofeed] subscribe n={} chart={}", syms.len(), if chart_on { chart.as_str() } else { "-" });
                                let mut s = sub.lock().unwrap();
                                s.gen += 1;
                                s.chart = (chart_on && !chart.trim().is_empty()).then(|| chart.trim().to_string());
                                s.symbols = syms;
                            }
                            acc.drain(..4 + len);
                        }
                    }
                }
            }
            closed.store(true, Ordering::Relaxed);
        });
    }
    w.write_all(&frame(vec![1, 1]))?; // hello ready
    let mut states: std::collections::HashMap<String, St> = Default::default();
    let mut rng = Rng(42);
    let mut seen_gen = 0u64;
    let mut tick = 0u64;
    while !closed.load(Ordering::Relaxed) && current.load(Ordering::Relaxed) == my_gen {
        let (syms, chart, gen) = {
            let s = sub.lock().unwrap();
            (s.symbols.clone(), s.chart.clone(), s.gen)
        };
        if gen != seen_gen {
            seen_gen = gen;
            if let Some(c) = &chart {
                let st = states.entry(c.clone()).or_insert_with(|| state_for(c));
                w.write_all(&intraday(c, st))?;
            }
        }
        if syms.is_empty() {
            thread::sleep(Duration::from_millis(100));
            continue;
        }
        let mut body = vec![2u8];
        body.extend_from_slice(&(syms.len() as u16).to_le_bytes());
        for s in &syms {
            let st = states.entry(s.clone()).or_insert_with(|| state_for(s));
            let wobble = (rng.unit() - 0.48) * (st.price * 0.0015).max(0.5);
            st.price = (st.price + wobble).max(0.01);
            st.volume += rng.range(10, 400);
            text(&mut body, s);
            text(&mut body, name_of(s));
            body.extend_from_slice(&cents(st.price).to_le_bytes());
            body.extend_from_slice(&cents(st.price - st.prev).to_le_bytes());
            body.extend_from_slice(&st.volume.to_le_bytes());
        }
        w.write_all(&frame(body))?;
        tick += 1;
        if tick % 40 == 0 {
            if let Some(c) = &chart {
                if let Some(st) = states.get(c) {
                    let px = cents(st.price);
                    let mut b = vec![4u8];
                    text(&mut b, c);
                    b.extend_from_slice(&1u16.to_le_bytes());
                    bar(&mut b, minute_of_day(), px, px + 5, px - 5, px, rng.range(100, 5000));
                    w.write_all(&frame(b))?;
                }
            }
        }
        thread::sleep(interval);
    }
    Ok(())
}

fn main() {
    let v: Vec<String> = std::env::args().skip(1).collect();
    let mut addr = "127.0.0.1:47631".to_string();
    let mut interval_ms = 25u64;
    let mut i = 0;
    while i < v.len() {
        match v[i].as_str() {
            "--addr" => addr = v.get(i + 1).cloned().unwrap_or(addr),
            "--interval-ms" => interval_ms = v.get(i + 1).and_then(|x| x.parse().ok()).unwrap_or(25).max(1),
            "-h" | "--help" => {
                println!("xq-demofeed [--addr 127.0.0.1:47631] [--interval-ms 25]  （WryFeedHost --demo-ticks 的 Rust 版；合成行情，非真實資料）");
                return;
            }
            _ => {}
        }
        i += 2;
    }
    let listener = TcpListener::bind(&addr).unwrap_or_else(|e| panic!("bind {addr}: {e}"));
    println!("[xq-demofeed] listening {addr} interval={interval_ms}ms  *** 合成行情（路徑測試用），不是真實行情 ***");
    let current = Arc::new(AtomicU64::new(0));
    for conn in listener.incoming() {
        let Ok(stream) = conn else { continue };
        let gen = current.fetch_add(1, Ordering::Relaxed) + 1;
        println!("[xq-demofeed] client connected #{gen}");
        let current = current.clone();
        thread::spawn(move || {
            let r = serve(stream, gen, current, Duration::from_millis(interval_ms));
            println!("[xq-demofeed] client #{gen} done {:?}", r.err().map(|e| e.to_string()));
        });
    }
}

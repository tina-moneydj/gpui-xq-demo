//! xq-tickgen：高頻進價壓測用的行情產生器，講和 WryFeedHost 一樣的協議（127.0.0.1:47631）。
//!
//! - 一次服務一個 client（同 WryFeedHost）；連上先送 hello(ready)，等第一個訂閱包＋`--settle` 秒，
//!   送一次全部代號的快照（33 檔自選＋`--rows` 檔 Z00000…，不計入壓測筆數），再等 `--warmup` 秒開始。
//! - 每 `--frame-ms`（預設 5ms）送一包，筆數照排程補齊：穩定 `--rate N` 筆/秒，或
//!   `--burst peak,tau,base` 開盤爆量：rate(t) = base + peak·e^(−t/tau)。**不合併**，每筆都送。
//! - 每包第一筆是探針代號（`--probe`，預設 2330，走勢圖那檔），其餘依 `--hot` 機率落在自選，否則均勻落在 Z 列。
//! - 每包附壓測尾巴 `XQTS + seq + sent_us`（見 xq_feed::encode_quotes），程式在 XQ_TICK_BENCH=1 時算端到端延遲。
//! - 價格：圍繞昨收的均值回歸隨機走（±10% 漲跌停夾住），不會漂移。
//! - stdout 每行一個 JSON（listen / client / sub / snapshot / start / sec / end），給 bench 腳本解析。
//!
//! 用法：xq-tickgen [--addr 127.0.0.1:47631] [--rate 10000 | --burst 150000,3,5000] [--duration 30]
//!                   [--settle 3] [--warmup 3] [--frame-ms 5] [--rows 50000] [--hot 0.2] [--probe 2330] [--seed 1]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use xq_feed::{encode_quotes, unix_us, Quote};

const WATCH: &[(&str, &str, f64)] = &[
    ("2330", "台積電", 1035.0), ("2317", "鴻海", 198.5), ("2454", "聯發科", 1280.0), ("0050", "元大台灣50", 186.2),
    ("2303", "聯電", 48.6), ("2881", "富邦金", 88.4), ("2882", "國泰金", 62.3), ("2308", "台達電", 372.0),
    ("2412", "中華電", 126.5), ("2382", "廣達", 285.0), ("3711", "日月光投控", 158.0), ("2891", "中信金", 36.2),
    ("2886", "兆豐金", 40.1), ("1301", "台塑", 52.8), ("1303", "南亞", 46.3), ("2002", "中鋼", 22.9),
    ("3008", "大立光", 2280.0), ("2357", "華碩", 545.0), ("2603", "長榮", 198.0), ("2609", "陽明", 68.5),
    ("3231", "緯創", 112.0), ("6505", "台塑化", 48.2), ("2884", "玉山金", 27.4), ("2892", "第一金", 27.9),
    ("5880", "合庫金", 26.6), ("2207", "和泰車", 560.0), ("1216", "統一", 78.9), ("2379", "瑞昱", 512.0),
    ("3034", "聯詠", 498.0), ("2345", "智邦", 620.0), ("AAPL", "Apple", 228.0), ("TSLA", "Tesla", 245.0),
    ("NVDA", "NVIDIA", 132.0),
];

struct Args {
    addr: String,
    rate: f64,
    burst: Option<(f64, f64, f64)>,
    duration: f64,
    settle: f64,
    warmup: f64,
    frame_ms: u64,
    rows: usize,
    hot: f64,
    probe: String,
    seed: u64,
}

fn parse_args() -> Args {
    let mut a = Args {
        addr: "127.0.0.1:47631".into(),
        rate: 10_000.0,
        burst: None,
        duration: 30.0,
        settle: 3.0,
        warmup: 3.0,
        frame_ms: 5,
        rows: 50_000,
        hot: 0.2,
        probe: "2330".into(),
        seed: 1,
    };
    let v: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < v.len() {
        let val = v.get(i + 1).cloned().unwrap_or_default();
        let f = || val.parse::<f64>().unwrap_or_else(|_| panic!("{} 需要數字", v[i]));
        match v[i].as_str() {
            "--addr" => a.addr = val.clone(),
            "--rate" => a.rate = f(),
            "--burst" => {
                let p: Vec<f64> = val.split(',').map(|x| x.trim().parse().expect("--burst peak,tau,base")).collect();
                assert!(p.len() == 3, "--burst peak,tau,base");
                a.burst = Some((p[0], p[1], p[2]));
            }
            "--duration" => a.duration = f(),
            "--settle" => a.settle = f(),
            "--warmup" => a.warmup = f(),
            "--frame-ms" => a.frame_ms = f().max(1.0) as u64,
            "--rows" => a.rows = f() as usize,
            "--hot" => a.hot = f().clamp(0.0, 1.0),
            "--probe" => a.probe = val.clone(),
            "--seed" => a.seed = f() as u64,
            "-h" | "--help" => {
                eprintln!("見原始碼開頭說明");
                std::process::exit(0);
            }
            other => panic!("不認得的參數 {other}"),
        }
        i += 2;
    }
    a
}

/// 累積排程：到 t 秒為止應送出的筆數
fn target(a: &Args, t: f64) -> f64 {
    match a.burst {
        Some((peak, tau, base)) => base * t + peak * tau * (1.0 - (-t / tau).exp()),
        None => a.rate * t,
    }
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        ((self.next() >> 11) as u128 * n as u128 >> 53) as usize
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

struct Sym {
    symbol: String,
    name: String,
    base: i64,  // 昨收（分）
    price: i64, // 現價（分）
    volume: i64,
}

impl Sym {
    /// 均值回歸隨機走：步長約 0.1% 昨收，離昨收越遠越往回拉，夾在 ±10%。
    fn step(&mut self, rng: &mut Rng) -> Quote {
        let tick = (self.base / 1000).max(1);
        let dev = (self.price - self.base) as f64 / self.base as f64; // −0.1..0.1
        let u = rng.unit() - 0.5 - dev * 4.0; // dev=5% 時偏 −0.2
        let mv = (u * 6.0).round() as i64 * tick;
        let lim = self.base / 10;
        self.price = (self.price + mv).clamp(self.base - lim, self.base + lim).max(1);
        self.volume += 1000 * (1 + rng.below(20) as i64);
        self.quote()
    }
    fn quote(&self) -> Quote {
        Quote {
            symbol: self.symbol.clone(),
            name: self.name.clone(),
            price: self.price as f64 / 100.0,
            change: (self.price - self.base) as f64 / 100.0,
            volume: self.volume,
        }
    }
}

fn universe(a: &Args) -> Vec<Sym> {
    let mut v: Vec<Sym> = WATCH
        .iter()
        .map(|(s, n, p)| {
            let base = (p * 100.0).round() as i64;
            Sym { symbol: (*s).into(), name: (*n).into(), base, price: base, volume: 1_000_000 }
        })
        .collect();
    for k in 0..a.rows {
        // 與 GPUI 版 toggle_stress 同一組合成昨收（10.00–909.99）
        let h = (k as u64).wrapping_mul(2654435761) % 100_000;
        let base = 1000 + (h % 90_000) as i64;
        v.push(Sym { symbol: format!("Z{k:05}"), name: "壓力測試".into(), base, price: base, volume: (h * 7) as i64 });
    }
    v
}

fn emit(line: String) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

fn main() {
    let a = parse_args();
    let listener = TcpListener::bind(&a.addr).unwrap_or_else(|e| panic!("bind {}: {e}", a.addr));
    emit(format!(
        "{{\"ev\":\"listen\",\"addr\":\"{}\",\"rate\":{},\"burst\":{},\"duration\":{},\"frameMs\":{},\"rows\":{},\"hot\":{}}}",
        a.addr,
        a.rate,
        a.burst.map(|(p, t, b)| format!("[{p},{t},{b}]")).unwrap_or_else(|| "null".into()),
        a.duration,
        a.frame_ms,
        a.rows,
        a.hot
    ));
    // 一次一個 client；斷線就結束（bench 腳本每輪重開產生器）
    let (stream, peer) = listener.accept().expect("accept");
    emit(format!("{{\"ev\":\"client\",\"peer\":\"{peer}\",\"unixUs\":{}}}", unix_us()));
    if let Err(e) = serve(&a, stream) {
        emit(format!("{{\"ev\":\"error\",\"msg\":\"{e}\"}}"));
        std::process::exit(1);
    }
}

fn serve(a: &Args, stream: TcpStream) -> std::io::Result<()> {
    stream.set_nodelay(true)?;
    let mut w = stream.try_clone()?;
    let subscribed = Arc::new(AtomicU64::new(0));
    let closed = Arc::new(AtomicBool::new(false));
    {
        // 讀訂閱包（內容不影響產生器：壓測一律送全部代號），EOF 就標記斷線
        let (subscribed, closed) = (subscribed.clone(), closed.clone());
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
                            if acc.len() < 4 + len {
                                break;
                            }
                            let p = &acc[4..4 + len];
                            let chart = p.get(3..3 + *p.get(2).unwrap_or(&0) as usize).map(String::from_utf8_lossy).unwrap_or_default();
                            emit(format!("{{\"ev\":\"sub\",\"bytes\":{len},\"chart\":\"{chart}\",\"unixUs\":{}}}", unix_us()));
                            subscribed.fetch_add(1, Ordering::Relaxed);
                            acc.drain(..4 + len);
                        }
                    }
                }
            }
            closed.store(true, Ordering::Relaxed);
            emit(format!("{{\"ev\":\"closed\",\"unixUs\":{}}}", unix_us()));
        });
    }
    w.write_all(&[2, 0, 0, 0, 1, 1])?; // hello ready

    let wait_until = Instant::now() + Duration::from_secs(60);
    while subscribed.load(Ordering::Relaxed) == 0 {
        if Instant::now() > wait_until || closed.load(Ordering::Relaxed) {
            return Err(std::io::Error::other("沒有收到訂閱"));
        }
        thread::sleep(Duration::from_millis(20));
    }
    thread::sleep(Duration::from_secs_f64(a.settle));

    let mut syms = universe(a);
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ a.seed.wrapping_mul(0xD1B5_4A32_D192_ED03));
    // 快照：全部代號各一筆（讓兩邊表格起始值一樣），不帶時間戳、不計入壓測
    let t_snap = Instant::now();
    for chunk in syms.chunks(2000) {
        let qs: Vec<Quote> = chunk.iter().map(Sym::quote).collect();
        w.write_all(&encode_quotes(&qs, None))?;
    }
    emit(format!("{{\"ev\":\"snapshot\",\"rows\":{},\"ms\":{:.1}}}", syms.len(), t_snap.elapsed().as_secs_f64() * 1000.0));
    thread::sleep(Duration::from_secs_f64(a.warmup));

    let probe = syms.iter().position(|s| s.symbol == a.probe).unwrap_or(0);
    let nwatch = WATCH.len();
    let nz = a.rows;
    let frame = Duration::from_millis(a.frame_ms);
    let per_frame_nominal = target(a, a.frame_ms as f64 / 1000.0).max(1.0);
    emit(format!("{{\"ev\":\"start\",\"unixUs\":{}}}", unix_us()));
    let t0 = Instant::now();
    let (mut sent, mut seq, mut frames, mut bytes) = (0u64, 0u64, 0u64, 0u64);
    let (mut max_due, mut write_us_total, mut write_us_max) = (0u64, 0u64, 0u64);
    let mut sec = 1u64;
    let (mut sec_sent, mut sec_frames, mut sec_max_due, mut sec_write_max, mut behind_frames) = (0u64, 0u64, 0u64, 0u64, 0u64);
    let mut next = t0;
    let mut qs: Vec<Quote> = Vec::new();
    loop {
        let now = Instant::now();
        let t = now.duration_since(t0).as_secs_f64();
        if t >= a.duration || closed.load(Ordering::Relaxed) {
            break;
        }
        let due = (target(a, t) as u64).saturating_sub(sent);
        max_due = max_due.max(due);
        sec_max_due = sec_max_due.max(due);
        if due as f64 > per_frame_nominal * 3.0 + 10.0 {
            behind_frames += 1;
        }
        let mut left = due;
        while left > 0 {
            let n = left.min(60_000) as usize;
            qs.clear();
            qs.push(syms[probe].step(&mut rng));
            for _ in 1..n {
                let i = if nz == 0 || rng.unit() < a.hot { rng.below(nwatch) } else { nwatch + rng.below(nz) };
                qs.push(syms[i].step(&mut rng));
            }
            seq += 1;
            let f = encode_quotes(&qs, Some((seq, unix_us())));
            let tw = Instant::now();
            w.write_all(&f)?;
            let wus = tw.elapsed().as_micros() as u64;
            write_us_total += wus;
            write_us_max = write_us_max.max(wus);
            sec_write_max = sec_write_max.max(wus);
            sent += n as u64;
            sec_sent += n as u64;
            frames += 1;
            sec_frames += 1;
            bytes += f.len() as u64;
            left -= n as u64;
        }
        let t_after = t0.elapsed().as_secs_f64();
        while t_after >= sec as f64 {
            emit(format!(
                "{{\"ev\":\"sec\",\"t\":{sec},\"target\":{:.0},\"sent\":{sent},\"secSent\":{sec_sent},\"frames\":{sec_frames},\"maxDue\":{sec_max_due},\"maxWriteMs\":{:.2}}}",
                target(a, sec as f64),
                sec_write_max as f64 / 1000.0
            ));
            sec += 1;
            sec_sent = 0;
            sec_frames = 0;
            sec_max_due = 0;
            sec_write_max = 0;
        }
        next += frame;
        let now = Instant::now();
        if next > now {
            thread::sleep(next - now);
        } else {
            next = now; // 落後就不補睡，下一輪 due 會把欠的筆數一次補上
        }
    }
    let elapsed = t0.elapsed().as_secs_f64();
    emit(format!(
        "{{\"ev\":\"end\",\"unixUs\":{},\"elapsed\":{elapsed:.3},\"target\":{:.0},\"sent\":{sent},\"frames\":{frames},\"bytes\":{bytes},\"lastSeq\":{seq},\"maxDue\":{max_due},\"behindFrames\":{behind_frames},\"writeMsTotal\":{:.1},\"maxWriteMs\":{:.2}}}",
        unix_us(),
        target(a, elapsed.min(a.duration)),
        write_us_total as f64 / 1000.0,
        write_us_max as f64 / 1000.0
    ));
    // 跑完保持連線（不再送），讓 bench 腳本量收尾；client 斷線或被結束才離開
    while !closed.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

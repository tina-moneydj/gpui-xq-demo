//! WryFeedHost 行情協議（與 wry-xq-demo/src/engine.rs 同一份格式）。
//!
//! 封包：little-endian `u32 長度 + payload`，payload 第一個 byte 是型別：
//! - 1 hello   : `[1][ready u8]`
//! - 2 quotes  : `[2][n u16]{ sym, name, price i32(分), change i32(分), volume i64 }`
//! - 3 intraday: `[3][sym][prev i32][n u16]{ bar }`
//! - 4 minutes : `[4][sym][n u16]{ bar }`
//!
//! bar = `t u16(當日分鐘) o h l c i32(分) v i64`；文字 = `len u8 + utf8`。
//! 訂閱（client→host）：`[1][chart_on u8][chart text][n u16]{ sym text }`。
//!
//! 這個 crate 不依賴任何 UI 框架：`spawn_client` 在背景執行緒連線／重連，
//! 解碼後用 callback 交出 [`Event`]，由呼叫端決定怎麼合併、何時推給 UI。

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

pub const DEFAULT_PORT: u16 = 47631;
pub const MAX_SYMBOLS: usize = 64;
const MAX_FRAME: usize = 8_000_000;

/// 訂閱：報價代號 + 走勢圖代號 + 週期（"T" = 分時才會要分鐘線）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subscribe {
    pub symbols: Vec<String>,
    pub chart: String,
    pub period: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Quote {
    pub symbol: String,
    pub name: String,
    pub price: f64,
    pub change: f64,
    pub volume: i64,
}

/// 一根 K（分鐘或日）；價格已從「分」換成元。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bar {
    pub t: u16,
    pub o: f64,
    pub h: f64,
    pub l: f64,
    pub c: f64,
    pub v: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    Hello { ready: bool },
    Quotes(Vec<Quote>),
    Intraday { symbol: String, prev: f64, bars: Vec<Bar> },
    Minutes { symbol: String, bars: Vec<Bar> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Connected,
    Down,
    Msg(Msg),
    /// 高頻進價壓測（`XQ_TICK_BENCH=1` 才會出現）：緊接在帶時間戳的報價包之後送出。
    Stamp(Stamp),
}

/// 壓測時間戳尾巴：報價包（型別 2）在 n 筆報價之後多附 20 bytes
/// `b"XQTS" + seq u64 + sent_us u64`（產生器送出時的 Unix µs）。
/// 一般解碼器讀完 n 筆就停，會忽略這段尾巴，所以向下相容（WryFeedHost 不會送）。
pub const STAMP_MAGIC: [u8; 4] = *b"XQTS";
pub const STAMP_LEN: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub seq: u64,
    /// 產生器送出時間（Unix µs，CLOCK_REALTIME；同機比較）
    pub sent_us: u64,
    /// 背景執行緒 read() 回來的時間（Unix µs）
    pub recv_us: u64,
    /// 這一包的報價筆數（未合併）
    pub rows: u32,
}

/// 取出報價包尾端的壓測時間戳 `(seq, sent_us)`；沒有就回 None。
pub fn stamp_of(payload: &[u8]) -> Option<(u64, u64)> {
    if payload.first() != Some(&2) || payload.len() < 3 + STAMP_LEN {
        return None;
    }
    let t = &payload[payload.len() - STAMP_LEN..];
    if t[..4] != STAMP_MAGIC {
        return None;
    }
    Some((u64::from_le_bytes(t[4..12].try_into().ok()?), u64::from_le_bytes(t[12..20].try_into().ok()?)))
}

/// 現在的 Unix 時間（µs）
pub fn unix_us() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_micros() as u64).unwrap_or(0)
}

/// `XQ_TICK_BENCH=1`（非 0）才解析時間戳；預設關閉，熱路徑零成本。
pub fn tick_bench_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("XQ_TICK_BENCH").is_some_and(|v| v != "0"))
}

/// 報價包（含 u32 長度）；`stamp` = Some((seq, sent_us)) 時附壓測尾巴。給產生器／測試用。
pub fn encode_quotes(quotes: &[Quote], stamp: Option<(u64, u64)>) -> Vec<u8> {
    let mut body = Vec::with_capacity(3 + quotes.len() * 40 + STAMP_LEN);
    body.push(2);
    body.extend_from_slice(&(quotes.len().min(u16::MAX as usize) as u16).to_le_bytes());
    for q in quotes.iter().take(u16::MAX as usize) {
        push_text(&mut body, &q.symbol);
        push_text(&mut body, &q.name);
        body.extend_from_slice(&((q.price * 100.0).round() as i32).to_le_bytes());
        body.extend_from_slice(&((q.change * 100.0).round() as i32).to_le_bytes());
        body.extend_from_slice(&q.volume.to_le_bytes());
    }
    if let Some((seq, sent_us)) = stamp {
        body.extend_from_slice(&STAMP_MAGIC);
        body.extend_from_slice(&seq.to_le_bytes());
        body.extend_from_slice(&sent_us.to_le_bytes());
    }
    let mut frame = Vec::with_capacity(body.len() + 4);
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(&body);
    frame
}

pub fn encode_subscribe(cmd: &Subscribe) -> Vec<u8> {
    let chart_on = cmd.period.eq_ignore_ascii_case("T") && !cmd.chart.is_empty();
    let mut body = Vec::with_capacity(64);
    body.push(1);
    body.push(chart_on as u8);
    push_text(&mut body, &cmd.chart);
    let n = cmd.symbols.len().min(MAX_SYMBOLS);
    body.extend_from_slice(&(n as u16).to_le_bytes());
    for symbol in cmd.symbols.iter().take(n) {
        push_text(&mut body, symbol);
    }
    let mut frame = Vec::with_capacity(body.len() + 4);
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(&body);
    frame
}

fn push_text(body: &mut Vec<u8>, text: &str) {
    // 截在 96 bytes 內最近的 UTF-8 字元邊界，避免切壞中文。
    let mut n = text.len().min(96);
    while !text.is_char_boundary(n) {
        n -= 1;
    }
    body.push(n as u8);
    body.extend_from_slice(&text.as_bytes()[..n]);
}

/// 累積 TCP 位元組並切出完整 payload；半包保留到下次。
#[derive(Default)]
pub struct FrameReader {
    pending: Vec<u8>,
    start: usize,
}

impl FrameReader {
    pub fn push(&mut self, bytes: &[u8]) {
        if self.start > 0 && self.start == self.pending.len() {
            self.pending.clear();
            self.start = 0;
        }
        self.pending.extend_from_slice(bytes);
    }

    /// 回傳下一個完整 payload（借用內部緩衝，避免每包配置）。
    pub fn next_frame(&mut self) -> std::io::Result<Option<&[u8]>> {
        let avail = &self.pending[self.start..];
        if avail.len() < 4 {
            self.compact();
            return Ok(None);
        }
        let n = u32::from_le_bytes([avail[0], avail[1], avail[2], avail[3]]) as usize;
        if n == 0 || n > MAX_FRAME {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "bad frame"));
        }
        if avail.len() < 4 + n {
            self.compact();
            return Ok(None);
        }
        let begin = self.start + 4;
        self.start = begin + n;
        Ok(Some(&self.pending[begin..begin + n]))
    }

    fn compact(&mut self) {
        if self.start > 0 {
            self.pending.drain(..self.start);
            self.start = 0;
        }
    }
}

struct Cursor<'a> {
    buf: &'a [u8],
    i: usize,
}

impl<'a> Cursor<'a> {
    fn u8(&mut self) -> Option<u8> {
        let v = *self.buf.get(self.i)?;
        self.i += 1;
        Some(v)
    }
    fn u16(&mut self) -> Option<u16> {
        let b = self.buf.get(self.i..self.i + 2)?;
        self.i += 2;
        Some(u16::from_le_bytes([b[0], b[1]]))
    }
    fn i32(&mut self) -> Option<i32> {
        let b = self.buf.get(self.i..self.i + 4)?;
        self.i += 4;
        Some(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn i64(&mut self) -> Option<i64> {
        let b = self.buf.get(self.i..self.i + 8)?;
        self.i += 8;
        Some(i64::from_le_bytes(b.try_into().ok()?))
    }
    fn text(&mut self) -> Option<String> {
        let n = self.u8()? as usize;
        let b = self.buf.get(self.i..self.i + n)?;
        self.i += n;
        Some(String::from_utf8_lossy(b).into_owned())
    }
    fn px(&mut self) -> Option<f64> {
        Some(self.i32()? as f64 / 100.0)
    }
    fn bar(&mut self) -> Option<Bar> {
        Some(Bar { t: self.u16()?, o: self.px()?, h: self.px()?, l: self.px()?, c: self.px()?, v: self.i64()? })
    }
}

pub fn decode(payload: &[u8]) -> Option<Msg> {
    let mut c = Cursor { buf: payload, i: 1 };
    match *payload.first()? {
        1 => Some(Msg::Hello { ready: payload.get(1).copied().unwrap_or(0) == 1 }),
        2 => {
            let n = c.u16()? as usize;
            let mut quotes = Vec::with_capacity(n);
            for _ in 0..n {
                quotes.push(Quote {
                    symbol: c.text()?,
                    name: c.text()?,
                    price: c.px()?,
                    change: c.px()?,
                    volume: c.i64()?,
                });
            }
            Some(Msg::Quotes(quotes))
        }
        3 => {
            let symbol = c.text()?;
            let prev = c.px()?;
            let n = c.u16()? as usize;
            let mut bars = Vec::with_capacity(n);
            for _ in 0..n {
                bars.push(c.bar()?);
            }
            Some(Msg::Intraday { symbol, prev, bars })
        }
        4 => {
            let symbol = c.text()?;
            let n = c.u16()? as usize;
            let mut bars = Vec::with_capacity(n);
            for _ in 0..n {
                bars.push(c.bar()?);
            }
            Some(Msg::Minutes { symbol, bars })
        }
        _ => None,
    }
}

/// 位址：預設 127.0.0.1:47631；`XQ_FEED_ADDR=host:port` 可覆寫。
pub fn feed_addr() -> SocketAddr {
    std::env::var("XQ_FEED_ADDR")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| ([127, 0, 0, 1], DEFAULT_PORT).into())
}

/// 背景執行緒：連線、重連（500ms）、送最新訂閱、解碼後呼叫 `on_event`。
/// `on_event` 在背景執行緒上跑，應該只做 O(1) 的合併，不要碰 UI。
pub fn spawn_client(
    addr: SocketAddr,
    rx: Receiver<Subscribe>,
    mut on_event: impl FnMut(Event) + Send + 'static,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("xq-feed".into())
        .spawn(move || {
            let mut latest: Option<Subscribe> = None;
            loop {
                match TcpStream::connect_timeout(&addr, Duration::from_millis(400)) {
                    Ok(stream) => {
                        eprintln!("[xq-feed] 已連上 {addr}");
                        on_event(Event::Connected);
                        match session(stream, &rx, &mut latest, &mut on_event) {
                            Ok(true) => return,
                            Ok(false) => {}
                            Err(error) => eprintln!("[xq-feed] 連線中斷: {error}"),
                        }
                        on_event(Event::Down);
                    }
                    Err(_) => loop {
                        match rx.try_recv() {
                            Ok(cmd) => latest = Some(cmd),
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => return,
                        }
                    },
                }
                thread::sleep(Duration::from_millis(500));
            }
        })
        .expect("spawn xq-feed thread")
}

/// 回傳 Ok(true) 代表呼叫端已關閉（程式結束），不要再重連。
fn session(
    mut stream: TcpStream,
    rx: &Receiver<Subscribe>,
    latest: &mut Option<Subscribe>,
    on_event: &mut impl FnMut(Event),
) -> std::io::Result<bool> {
    stream.set_read_timeout(Some(Duration::from_millis(50)))?;
    stream.set_nodelay(true)?;
    if let Some(cmd) = latest.as_ref() {
        stream.write_all(&encode_subscribe(cmd))?;
    }
    let mut reader = FrameReader::default();
    let mut buf = vec![0u8; 64 * 1024];
    let bench = tick_bench_enabled();
    loop {
        loop {
            match rx.try_recv() {
                Ok(cmd) => {
                    stream.write_all(&encode_subscribe(&cmd))?;
                    *latest = Some(cmd);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Ok(true),
            }
        }
        match stream.read(&mut buf) {
            Ok(0) => return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "closed")),
            Ok(n) => {
                let recv_us = if bench { unix_us() } else { 0 };
                reader.push(&buf[..n]);
                while let Some(payload) = reader.next_frame()? {
                    let stamp = if bench { stamp_of(payload) } else { None };
                    if let Some(msg) = decode(payload) {
                        let rows = if let Msg::Quotes(q) = &msg { q.len() as u32 } else { 0 };
                        on_event(Event::Msg(msg));
                        if let Some((seq, sent_us)) = stamp {
                            on_event(Event::Stamp(Stamp { seq, sent_us, recv_us, rows }));
                        }
                    }
                }
            }
            Err(e) if matches!(e.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) => {}
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(payload: &[u8]) -> Vec<u8> {
        let mut f = (payload.len() as u32).to_le_bytes().to_vec();
        f.extend_from_slice(payload);
        f
    }

    #[test]
    fn subscribe_layout() {
        let f = encode_subscribe(&Subscribe { symbols: vec!["2330".into()], chart: "2330".into(), period: "T".into() });
        assert_eq!(&f[4..], &[1, 1, 4, b'2', b'3', b'3', b'0', 1, 0, 4, b'2', b'3', b'3', b'0']);
    }

    #[test]
    fn split_frames_and_quotes() {
        let mut p = vec![2u8, 1, 0];
        p.push(4);
        p.extend_from_slice(b"2330");
        let name = "台積電".as_bytes();
        p.push(name.len() as u8);
        p.extend_from_slice(name);
        p.extend_from_slice(&103500i32.to_le_bytes());
        p.extend_from_slice(&(-150i32).to_le_bytes());
        p.extend_from_slice(&42i64.to_le_bytes());
        let mut bytes = frame(&[1, 1]);
        bytes.extend(frame(&p));
        let mut r = FrameReader::default();
        // 故意拆成半包
        r.push(&bytes[..7]);
        assert_eq!(decode(r.next_frame().unwrap().unwrap()), Some(Msg::Hello { ready: true }));
        assert!(r.next_frame().unwrap().is_none());
        r.push(&bytes[7..]);
        match decode(r.next_frame().unwrap().unwrap()) {
            Some(Msg::Quotes(q)) => {
                assert_eq!(q[0].name, "台積電");
                assert_eq!(q[0].price, 1035.0);
                assert_eq!(q[0].change, -1.5);
                assert_eq!(q[0].volume, 42);
            }
            other => panic!("{other:?}"),
        }
        assert!(r.next_frame().unwrap().is_none());
    }

    #[test]
    fn stamp_trailer_is_backward_compatible() {
        let q = Quote { symbol: "2330".into(), name: "台積電".into(), price: 1035.0, change: -1.5, volume: 42 };
        let f = encode_quotes(std::slice::from_ref(&q), Some((7, 123_456)));
        let payload = &f[4..];
        assert_eq!(stamp_of(payload), Some((7, 123_456)));
        // 一般解碼忽略尾巴
        assert_eq!(decode(payload), Some(Msg::Quotes(vec![q.clone()])));
        let plain = encode_quotes(&[q], None);
        assert_eq!(stamp_of(&plain[4..]), None);
    }
}

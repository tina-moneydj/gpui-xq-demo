//! 原生／瀏覽器（wasm32）差異集中在這裡；其餘程式碼兩邊共用。
//!
//! - `Instant`：wasm32-unknown-unknown 的 `std::time::Instant::now()` 會 panic，改用 web-time（底層 performance.now()）。
//! - `var` / `var_os`：原生讀環境變數；網頁版讀網址參數（`index.html?XQ_CHART_STRESS=1&XQ_BENCH=1`），同一組旗標。
//! - `unix_us`：原生 SystemTime；網頁版 performance.now()，第一次呼叫時對齊 Date.now()（同一個牆上時鐘，誤差 ≤1ms）。
//! - `spawn_feed`：原生＝xq-feed 的 TCP 背景執行緒；網頁版＝WebSocket（xq-ws-bridge 轉送同一份位元組），
//!   切包／解碼都用 xq-feed 的 `FrameReader` / `decode`，跟原生同一份程式碼。
//! - `log`：原生 stderr；網頁版 console.log（壓測腳本從 DevTools Protocol 收）。

#![allow(dead_code)]

use xq_feed::{Event, Subscribe};

#[cfg(not(target_arch = "wasm32"))]
pub use std::time::Instant;
#[cfg(target_arch = "wasm32")]
pub use web_time::Instant;

pub const IS_WEB: bool = cfg!(target_arch = "wasm32");

#[cfg(not(target_arch = "wasm32"))]
pub fn var(k: &str) -> Result<String, std::env::VarError> {
    std::env::var(k)
}
#[cfg(not(target_arch = "wasm32"))]
pub fn var_os(k: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(k)
}

#[cfg(target_arch = "wasm32")]
fn query() -> &'static std::collections::HashMap<String, String> {
    static Q: std::sync::OnceLock<std::collections::HashMap<String, String>> = std::sync::OnceLock::new();
    Q.get_or_init(|| {
        let mut m = std::collections::HashMap::new();
        let search = web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default();
        if let Ok(p) = web_sys::UrlSearchParams::new_with_str(&search) {
            let it = js_sys::try_iter(&p.entries()).ok().flatten();
            for e in it.into_iter().flatten().flatten() {
                let pair = js_sys::Array::from(&e);
                if let (Some(k), Some(v)) = (pair.get(0).as_string(), pair.get(1).as_string()) {
                    m.insert(k, v);
                }
            }
        }
        m
    })
}
#[cfg(target_arch = "wasm32")]
pub fn var(k: &str) -> Result<String, std::env::VarError> {
    query().get(k).cloned().ok_or(std::env::VarError::NotPresent)
}
#[cfg(target_arch = "wasm32")]
pub fn var_os(k: &str) -> Option<std::ffi::OsString> {
    query().get(k).map(|v| v.into())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn unix_us() -> u64 {
    xq_feed::unix_us()
}
/// 網頁版：performance.now()（單調、µs 級）＋第一次呼叫時對齊 Date.now()（牆上時鐘，跟產生器的 SystemTime 同一個時鐘）。
/// 不直接用 performance.timeOrigin：實測這台 VM 上 timeOrigin＋now() 可以跟牆上時鐘差到 20 幾秒（單調時鐘與 realtime 漂移）。
#[cfg(target_arch = "wasm32")]
pub fn unix_us() -> u64 {
    thread_local! {
        static OFFSET_MS: std::cell::Cell<Option<f64>> = const { std::cell::Cell::new(None) };
    }
    let Some(p) = web_sys::window().and_then(|w| w.performance()) else { return (js_sys::Date::now() * 1000.0) as u64 };
    let now = p.now();
    let off = OFFSET_MS.with(|o| match o.get() {
        Some(v) => v,
        None => {
            let v = js_sys::Date::now() - now;
            o.set(Some(v));
            v
        }
    });
    ((now + off) * 1000.0) as u64
}

pub fn tick_bench_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| var_os("XQ_TICK_BENCH").is_some_and(|v| v != "0"))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn log(s: &str) {
    eprintln!("{s}");
}
#[cfg(target_arch = "wasm32")]
pub fn log(s: &str) {
    web_sys::console::log_1(&s.into());
}

/// 狀態列上顯示的行情位址
#[cfg(not(target_arch = "wasm32"))]
pub fn feed_label() -> String {
    xq_feed::feed_addr().to_string()
}
#[cfg(target_arch = "wasm32")]
pub fn feed_label() -> String {
    ws_url()
}

/// 行情連線的「送訂閱」端
pub struct FeedTx {
    #[cfg(not(target_arch = "wasm32"))]
    tx: std::sync::mpsc::Sender<Subscribe>,
    #[cfg(target_arch = "wasm32")]
    ws: std::rc::Rc<web::Ws>,
}

impl FeedTx {
    pub fn send(&self, sub: Subscribe) {
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.tx.send(sub);
        #[cfg(target_arch = "wasm32")]
        self.ws.subscribe(sub);
    }
}

/// 開行情連線。`on_event` 原生在背景執行緒上跑、網頁版在主執行緒（WebSocket onmessage）上跑；都只做 O(1) 合併。
#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_feed(on_event: impl FnMut(Event) + Send + 'static) -> FeedTx {
    let (tx, rx) = std::sync::mpsc::channel();
    xq_feed::spawn_client(xq_feed::feed_addr(), rx, on_event);
    FeedTx { tx }
}
#[cfg(target_arch = "wasm32")]
pub fn spawn_feed(on_event: impl FnMut(Event) + 'static) -> FeedTx {
    FeedTx { ws: web::Ws::start(ws_url(), Box::new(on_event)) }
}

/// 預設 ws://<同一個 host>:47632（xq-ws-bridge 同時提供網頁檔案與行情）；`?XQ_FEED_WS=ws://…` 可覆寫
#[cfg(target_arch = "wasm32")]
pub fn ws_url() -> String {
    if let Ok(u) = var("XQ_FEED_WS") {
        return u;
    }
    let host = web_sys::window().and_then(|w| w.location().host().ok()).unwrap_or_else(|| "127.0.0.1:47632".into());
    format!("ws://{host}/feed")
}

/// wasm 線性記憶體大小（MB）：網頁版狀態列的「記憶體」用這個（瀏覽器行程的 RSS 由壓測腳本從外面量）
#[cfg(target_arch = "wasm32")]
pub fn wasm_heap_mb() -> f64 {
    use wasm_bindgen::JsCast;
    wasm_bindgen::memory().dyn_into::<js_sys::WebAssembly::Memory>().map(|m| m.buffer().dyn_into::<js_sys::ArrayBuffer>().map(|b| b.byte_length() as f64).unwrap_or(0.0)).unwrap_or(0.0)
        / 1048576.0
}

#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::RefCell;
    use std::rc::Rc;

    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;
    use web_sys::{BinaryType, MessageEvent, WebSocket};
    use xq_feed::{decode, encode_subscribe, stamp_of, Event, FrameReader, Msg, Stamp, Subscribe};

    type OnEvent = Box<dyn FnMut(Event)>;

    pub struct Ws {
        url: String,
        sock: RefCell<Option<WebSocket>>,
        open: RefCell<bool>,
        latest: RefCell<Option<Subscribe>>,
        on_event: RefCell<OnEvent>,
        reader: RefCell<FrameReader>,
        // 閉包要活著（JS 端持有）
        _cbs: RefCell<Vec<Closure<dyn FnMut(JsValue)>>>,
    }

    impl Ws {
        pub fn start(url: String, on_event: OnEvent) -> Rc<Ws> {
            let ws = Rc::new(Ws {
                url,
                sock: RefCell::new(None),
                open: RefCell::new(false),
                latest: RefCell::new(None),
                on_event: RefCell::new(on_event),
                reader: RefCell::new(FrameReader::default()),
                _cbs: RefCell::new(Vec::new()),
            });
            Ws::connect(&ws);
            ws
        }

        fn connect(this: &Rc<Ws>) {
            let Ok(sock) = WebSocket::new(&this.url) else { return Ws::retry(this) };
            sock.set_binary_type(BinaryType::Arraybuffer);
            let mut cbs = this._cbs.borrow_mut();
            cbs.clear();
            let me = Rc::downgrade(this);
            let on_open = Closure::<dyn FnMut(JsValue)>::new(move |_| {
                let Some(me) = me.upgrade() else { return };
                *me.open.borrow_mut() = true;
                *me.reader.borrow_mut() = FrameReader::default();
                crate::plat::log(&format!("[xq-feed] 已連上 {}", me.url));
                (me.on_event.borrow_mut())(Event::Connected);
                let sub = me.latest.borrow().clone();
                if let (Some(s), Some(sub)) = (me.sock.borrow().as_ref(), sub) {
                    let _ = s.send_with_u8_array(&encode_subscribe(&sub));
                };
            });
            let me = Rc::downgrade(this);
            let on_msg = Closure::<dyn FnMut(JsValue)>::new(move |e: JsValue| {
                let Some(me) = me.upgrade() else { return };
                let Ok(e) = e.dyn_into::<MessageEvent>() else { return };
                let Ok(buf) = e.data().dyn_into::<js_sys::ArrayBuffer>() else { return };
                let bench = crate::plat::tick_bench_enabled();
                let recv_us = if bench { crate::plat::unix_us() } else { 0 };
                let bytes = js_sys::Uint8Array::new(&buf).to_vec();
                let mut reader = me.reader.borrow_mut();
                reader.push(&bytes);
                let mut on_event = me.on_event.borrow_mut();
                loop {
                    match reader.next_frame() {
                        Ok(Some(payload)) => {
                            let stamp = if bench { stamp_of(payload) } else { None };
                            if let Some(msg) = decode(payload) {
                                let rows = if let Msg::Quotes(q) = &msg { q.len() as u32 } else { 0 };
                                on_event(Event::Msg(msg));
                                if let Some((seq, sent_us)) = stamp {
                                    on_event(Event::Stamp(Stamp { seq, sent_us, recv_us, rows }));
                                }
                            }
                        }
                        Ok(None) => break,
                        Err(_) => {
                            *reader = FrameReader::default();
                            break;
                        }
                    }
                }
            });
            let me = Rc::downgrade(this);
            let on_close = Closure::<dyn FnMut(JsValue)>::new(move |_| {
                let Some(me) = me.upgrade() else { return };
                let was_open = std::mem::replace(&mut *me.open.borrow_mut(), false);
                if was_open {
                    crate::plat::log("[xq-feed] 連線中斷");
                    (me.on_event.borrow_mut())(Event::Down);
                }
                Ws::retry(&me);
            });
            sock.set_onopen(Some(on_open.as_ref().unchecked_ref()));
            sock.set_onmessage(Some(on_msg.as_ref().unchecked_ref()));
            sock.set_onclose(Some(on_close.as_ref().unchecked_ref()));
            cbs.push(on_open);
            cbs.push(on_msg);
            cbs.push(on_close);
            drop(cbs);
            *this.sock.borrow_mut() = Some(sock);
        }

        /// 500ms 後重連（同原生）
        fn retry(this: &Rc<Ws>) {
            let me = Rc::downgrade(this);
            let cb = Closure::once_into_js(move || {
                if let Some(me) = me.upgrade() {
                    Ws::connect(&me);
                }
            });
            if let Some(w) = web_sys::window() {
                let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(cb.unchecked_ref(), 500);
            }
        }

        pub fn subscribe(&self, sub: Subscribe) {
            if *self.open.borrow() {
                if let Some(s) = self.sock.borrow().as_ref() {
                    let _ = s.send_with_u8_array(&encode_subscribe(&sub));
                }
            }
            *self.latest.borrow_mut() = Some(sub);
        }
    }
}

/// 股名／代號搜尋框（ime-search-test 分支併進來）：原生預設不顯示（三方壓測畫面不變），網頁版預設顯示；
/// `XQ_SEARCH=0/1` 可強制。網頁版壓測帶 `XQ_SEARCH=0`，跟原生同畫面。
pub fn search_enabled() -> bool {
    var("XQ_SEARCH").map(|v| v != "0").unwrap_or(IS_WEB)
}

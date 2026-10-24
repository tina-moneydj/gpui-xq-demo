//! 背景行情執行緒 → UI 的合併層。
//!
//! 背景執行緒（xq-feed）每收到一包就在 `Inbox` 裡 O(1) 合併：
//! 同代號的報價只留最新一筆、分時快照只留最新、分鐘線依序累積。
//! 只有 inbox 從「空」變成「有東西」時才送一個喚醒訊號；UI 端被喚醒只標記「要畫下一幀」，
//! 在下一幀 root render 開頭整包取走（main.rs `Terminal::render`），
//! 所以不管引擎推多快，UI 最多每 frame 套用一次，而且套用完馬上畫在同一幀。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use parking_lot::Mutex;
use xq_feed::{Bar, Event, Msg, Quote, Stamp, Subscribe};

use crate::tick_tape::TapeFeed;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conn {
    Connecting,
    Up,
    Ready,
    Down,
}

#[derive(Default)]
pub struct Inbox {
    pub quotes: HashMap<String, Quote>,
    /// 插入順序（第一次出現），讓套用順序穩定
    pub order: Vec<String>,
    pub intraday: Option<(String, f64, Vec<Bar>)>,
    pub minutes: Vec<(String, Bar)>,
    pub conn: Option<Conn>,
    /// XQ_TICK_BENCH=1：這批裡最新一包的壓測時間戳（每包都含探針代號，所以只留最新）
    pub stamp: Option<Stamp>,
    /// 同上，這批裡最早一包（= 這批最舊的 tick，量「最久等多久」）
    pub stamp_first: Option<Stamp>,
}

pub struct Counters {
    /// 收到的封包數
    pub frames: AtomicU64,
    /// 收到的報價筆數（未合併）
    pub quote_rows: AtomicU64,
}

pub struct FeedBridge {
    pub inbox: Arc<Mutex<Inbox>>,
    pending: Arc<AtomicBool>,
    pub counters: Arc<Counters>,
    cmd: crate::plat::FeedTx,
    last_sub: Option<Subscribe>,
}

impl FeedBridge {
    /// `tape`：有開成交明細面板（`XQ_TAPE=1`）才給；None 時熱路徑完全不多做事。
    pub fn start(tape: Option<TapeFeed>) -> (Self, UnboundedReceiver<()>) {
        let inbox = Arc::new(Mutex::new(Inbox::default()));
        let pending = Arc::new(AtomicBool::new(false));
        let counters = Arc::new(Counters { frames: AtomicU64::new(0), quote_rows: AtomicU64::new(0) });
        let (wake_tx, wake_rx) = unbounded::<()>();
        let cmd_tx = {
            let inbox = inbox.clone();
            let pending = pending.clone();
            let counters = counters.clone();
            crate::plat::spawn_feed(move |ev| {
                merge(&inbox, &counters, tape.as_ref(), ev);
                wake(&pending, &wake_tx);
            })
        };
        (FeedBridge { inbox, pending, counters, cmd: cmd_tx, last_sub: None }, wake_rx)
    }

    /// UI 端：先清旗標再取走，之後到的包會再喚醒一次。
    pub fn take(&self) -> Inbox {
        self.pending.store(false, Ordering::Release);
        std::mem::take(&mut *self.inbox.lock())
    }

    /// 背景有新東西還沒被 UI 取走（render 套用／後備套用用）
    pub fn pending(&self) -> bool {
        self.pending.load(Ordering::Acquire)
    }

    pub fn subscribe(&mut self, sub: Subscribe) {
        if self.last_sub.as_ref() == Some(&sub) {
            return;
        }
        self.last_sub = Some(sub.clone());
        self.cmd.send(sub);
    }
}

fn wake(pending: &AtomicBool, tx: &UnboundedSender<()>) {
    if !pending.swap(true, Ordering::AcqRel) {
        let _ = tx.unbounded_send(());
    }
}

fn merge(inbox: &Mutex<Inbox>, counters: &Counters, tape: Option<&TapeFeed>, ev: Event) {
    let mut ib = inbox.lock();
    match ev {
        Event::Connected => ib.conn = Some(Conn::Up),
        Event::Down => {
            // 斷線：丟掉還沒套用的行情，避免殘包晚到
            let conn = Some(Conn::Down);
            *ib = Inbox { conn, ..Inbox::default() };
        }
        Event::Stamp(st) => {
            if ib.stamp_first.is_none() {
                ib.stamp_first = Some(st);
            }
            ib.stamp = Some(st);
        }
        Event::Msg(msg) => {
            counters.frames.fetch_add(1, Ordering::Relaxed);
            match msg {
                Msg::Hello { ready } => {
                    if ready {
                        ib.conn = Some(Conn::Ready);
                    }
                }
                Msg::Quotes(quotes) => {
                    counters.quote_rows.fetch_add(quotes.len() as u64, Ordering::Relaxed);
                    // 成交明細要「每一筆」，所以在合併（同代號只留最新）之前先看一眼
                    if let Some(t) = tape {
                        t.on_quotes(&quotes);
                    }
                    for q in quotes {
                        if !ib.quotes.contains_key(&q.symbol) {
                            ib.order.push(q.symbol.clone());
                        }
                        ib.quotes.insert(q.symbol.clone(), q);
                    }
                }
                Msg::Intraday { symbol, prev, bars } => {
                    // 新快照覆蓋之前未套用的分鐘線
                    ib.minutes.retain(|(s, _)| s != &symbol);
                    ib.intraday = Some((symbol, prev, bars));
                }
                Msg::Minutes { symbol, bars } => {
                    for b in bars {
                        // 同代號同分鐘只留最新
                        if let Some(last) = ib.minutes.last_mut() {
                            if last.0 == symbol && last.1.t == b.t {
                                last.1 = b;
                                continue;
                            }
                        }
                        ib.minutes.push((symbol.clone(), b));
                    }
                }
            }
        }
    }
}

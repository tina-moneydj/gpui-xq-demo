//! 高頻進價壓測量測（`XQ_TICK_BENCH=1` 才啟用，預設關閉；搭配 crates/xq-feed 的 xq-tickgen）。
//!
//! 延遲：產生器在每包報價尾端附 `seq + sent_us`（Unix µs）。背景執行緒記 read() 時間（recv），
//! UI 套用那一批時記 applied，root render 時用 `window.on_next_frame` 記「這一幀畫完後」的時間（present）。
//! on_next_frame 在下一次 frame 回呼才跑，所以 present 是上界（最多多算約 1 幀）。
//! 每秒把樣本與計數印成一行 `TICKBENCH|{json}`（stderr），bench 腳本解析。

use std::cell::RefCell;
use std::rc::Rc;

use xq_feed::{unix_us, Stamp};

#[derive(Default)]
pub struct TickBench {
    /// [seq_first, sent_first, seq_last, sent_last, recv_last, applied_us, present_us]
    samples: Rc<RefCell<Vec<[u64; 7]>>>,
    /// 點報價表切換代號：[handled_us, present_us]（click 時間由 bench 腳本記）
    switches: Rc<RefCell<Vec<(String, u64, u64)>>>,
    pending: Option<(Stamp, Stamp, u64)>,
    switch_pending: Option<(String, u64)>,
    /// 從 inbox 取出的列數（背景合併後）／表格實際有變的列數
    pub inbox_rows: u64,
    pub applied_rows: u64,
    pub applies: u64,
    pub apply_us: u64,
}

impl TickBench {
    pub fn on_apply(&mut self, first: Option<Stamp>, stamp: Option<Stamp>, inbox_rows: usize, changed: usize, apply_us: u64) {
        self.inbox_rows += inbox_rows as u64;
        self.applied_rows += changed as u64;
        self.applies += 1;
        self.apply_us += apply_us;
        if let Some(st) = stamp {
            let first = first.unwrap_or(st);
            // 同一幀內套用多批：保留最早那批的 first、最新的 last
            let first = match self.pending { Some((f, _, _)) => f, None => first };
            self.pending = Some((first, st, unix_us()));
        }
    }

    pub fn on_switch(&mut self, symbol: &str) {
        self.switch_pending = Some((symbol.to_string(), unix_us()));
    }

    /// root render 呼叫：有待量的樣本就掛 on_next_frame
    pub fn on_render(&mut self, window: &mut gpui::Window) {
        if let Some((f, st, applied)) = self.pending.take() {
            let samples = self.samples.clone();
            window.on_next_frame(move |_, _| {
                samples.borrow_mut().push([f.seq, f.sent_us, st.seq, st.sent_us, st.recv_us, applied, unix_us()]);
            });
        }
        if let Some((sym, handled)) = self.switch_pending.take() {
            let switches = self.switches.clone();
            window.on_next_frame(move |_, _| switches.borrow_mut().push((sym, handled, unix_us())));
        }
    }

    /// 每秒一行；計數為累計值
    pub fn report(&mut self, rows_in: u64, frames_in: u64, fps: u32, rss_mb: f64) {
        let lat: Vec<String> = self
            .samples
            .borrow_mut()
            .drain(..)
            .map(|s| format!("[{},{},{},{},{},{},{}]", s[0], s[1], s[2], s[3], s[4], s[5], s[6]))
            .collect();
        let sw: Vec<String> =
            self.switches.borrow_mut().drain(..).map(|(s, h, p)| format!("[\"{s}\",{h},{p}]")).collect();
        eprintln!(
            "TICKBENCH|{{\"now\":{},\"fps\":{fps},\"rssMb\":{rss_mb:.1},\"rowsIn\":{rows_in},\"framesIn\":{frames_in},\"inboxRows\":{},\"applied\":{},\"applies\":{},\"applyUs\":{},\"lat\":[{}],\"sw\":[{}]}}",
            unix_us(),
            self.inbox_rows,
            self.applied_rows,
            self.applies,
            self.apply_us,
            lat.join(","),
            sw.join(",")
        );
    }
}

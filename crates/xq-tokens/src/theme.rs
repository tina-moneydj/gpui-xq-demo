//! 型別化的 Theme：每個欄位對應一個 DTCG token 路徑。也被 build.rs 拿去在編譯時驗證「每個主題都有每個 token」。
//! 新增 token：在下面的 group! 加一行（欄位名 : 種類 = "token 路徑"），JSON 也加上同名 token。

use std::collections::BTreeMap;

use crate::dtcg::{Rgba, Tok};

struct Ctx<'a> {
    m: &'a BTreeMap<String, Tok>,
    errs: Vec<String>,
}

impl Ctx<'_> {
    fn color(&mut self, p: &str) -> Rgba {
        match self.m.get(p) {
            Some(Tok::Color(c)) => Some(*c),
            Some(_) => self.err(p, "不是 color"),
            None => self.err(p, "缺少"),
        }
        .unwrap_or(Rgba { rgb: 0xff00ff, a: 1.0 })
    }
    fn rgb(&mut self, p: &str) -> u32 {
        self.color(p).rgb
    }
    fn px(&mut self, p: &str) -> f32 {
        match self.m.get(p) {
            Some(Tok::Px(v)) => Some(*v),
            Some(Tok::Number(v)) => Some(*v as f32),
            Some(_) => self.err(p, "不是 dimension"),
            None => self.err(p, "缺少"),
        }
        .unwrap_or(0.0)
    }
    fn font(&mut self, p: &str) -> &'static str {
        match self.m.get(p) {
            Some(Tok::Font(s)) => Some(&*Box::leak(s.clone().into_boxed_str())),
            Some(_) => self.err(p, "不是 fontFamily"),
            None => self.err(p, "缺少"),
        }
        .unwrap_or("sans-serif")
    }
    fn palette(&mut self, prefix: &str, n: usize, two_digit: bool) -> Vec<u32> {
        (1..=n).map(|i| self.rgb(&if two_digit { format!("{prefix}.{i:02}") } else { format!("{prefix}.{i}") })).collect()
    }
    fn err<T>(&mut self, p: &str, why: &str) -> Option<T> {
        self.errs.push(format!("token「{p}」{why}"));
        None
    }
}

macro_rules! group {
    ($(#[$m:meta])* $name:ident { $($(#[$fm:meta])* $field:ident : $kind:ident $( ( $($arg:expr),* ) )? = $path:literal),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Debug)]
        pub struct $name { $($(#[$fm])* pub $field: group!(@ty $kind)),* }
        impl $name {
            fn load(c: &mut Ctx) -> Self { Self { $($field: c.$kind($path $(, $($arg),*)?)),* } }
        }
    };
    (@ty color) => { Rgba };
    (@ty rgb) => { u32 };
    (@ty px) => { f32 };
    (@ty font) => { &'static str };
    (@ty palette) => { Vec<u32> };
}

group!(
    /// 介面基本色
    Ui {
        bg: rgb = "color.ui.bg",
        panel: rgb = "color.ui.panel",
        header: rgb = "color.ui.header",
        border: rgb = "color.ui.border",
        text: rgb = "color.ui.text",
        text_dim: rgb = "color.ui.textDim",
        accent: rgb = "color.ui.accent",
        selected: rgb = "color.ui.selected",
    }
);

group!(
    /// 行情（漲跌平）
    Market {
        up: rgb = "color.market.up",
        down: rgb = "color.market.down",
        flat: rgb = "color.market.flat",
        up_flash: color = "color.market.upFlash",
        down_flash: color = "color.market.downFlash",
        volume: rgb = "color.market.volume",
        big_trade_bg: color = "color.market.bigTradeBg",
    }
);

group!(
    /// 走勢圖／K 線
    Chart {
        up: rgb = "color.chart.up",
        down: rgb = "color.chart.down",
        flat: rgb = "color.chart.flat",
        grid: rgb = "color.chart.grid",
        axis: rgb = "color.chart.axis",
        text: rgb = "color.chart.text",
        separator: rgb = "color.chart.separator",
        crosshair: rgb = "color.chart.crosshair",
        crosshair_label: rgb = "color.chart.crosshairLabel",
        midline: rgb = "color.chart.midline",
        prev_close: rgb = "color.chart.prevClose",
        area_fill: color = "color.chart.areaFill",
        tag_text: rgb = "color.chart.tagText",
        tag_bg: rgb = "color.chart.tagBg",
        tooltip_bg: color = "color.chart.tooltipBg",
        tooltip_border: rgb = "color.chart.tooltipBorder",
        label_bg_hline: rgb = "color.chart.labelBgHline",
        overlay: palette(20, true) = "color.chart.overlay",
        sub_vol: palette(2, false) = "color.chart.sub.vol",
        sub_kd: palette(2, false) = "color.chart.sub.kd",
        sub_macd: palette(2, false) = "color.chart.sub.macd",
        sub_rsi: palette(1, false) = "color.chart.sub.rsi",
        sub_wr: palette(1, false) = "color.chart.sub.wr",
        sub_dmi: palette(3, false) = "color.chart.sub.dmi",
        sub_atr: palette(1, false) = "color.chart.sub.atr",
        sub_obv: palette(1, false) = "color.chart.sub.obv",
    }
);

group!(
    /// 左側畫線工具列
    Toolbar {
        tip_bg: rgb = "color.toolbar.tipBg",
        button_on_bg: rgb = "color.toolbar.buttonOnBg",
        button_hover: rgb = "color.toolbar.buttonHover",
        icon_on: rgb = "color.toolbar.iconOn",
        icon_off: rgb = "color.toolbar.iconOff",
        icon_disabled: color = "color.toolbar.iconDisabled",
        separator: rgb = "color.toolbar.separator",
    }
);

group!(
    /// 畫線
    Drawing {
        stroke: rgb = "color.drawing.stroke",
        selected: rgb = "color.drawing.selected",
        handle_border: rgb = "color.drawing.handleBorder",
        text_bg: color = "color.drawing.textBg",
        fib: palette(7, false) = "color.drawing.fib",
    }
);

group!(
    /// 字型與字級（px）
    Font {
        ui: font = "font.family.ui",
        mono: font = "font.family.mono",
        xs: px = "font.size.xs",
        sm: px = "font.size.sm",
        md: px = "font.size.md",
        lg: px = "font.size.lg",
        xl: px = "font.size.xl",
    }
);

group!(
    /// 間距（px）
    Space { xxs: px = "space.xxs", xs: px = "space.xs", sm: px = "space.sm", md: px = "space.md", lg: px = "space.lg" }
);

group!(
    /// 圓角（px）
    Radius { xs: px = "radius.xs", sm: px = "radius.sm", md: px = "radius.md" }
);

group!(
    /// 線寬（px）
    Stroke { hairline: px = "stroke.hairline", selected: px = "stroke.selected", series: px = "stroke.series" }
);

group!(
    /// 版面尺寸（px）
    Layout {
        title_bar: px = "layout.titleBar",
        status_bar: px = "layout.statusBar",
        splitter: px = "layout.splitter",
        table_row: px = "layout.tableRow",
        table_header: px = "layout.tableHeader",
        table_col_header: px = "layout.tableColHeader",
        tape_row: px = "layout.tapeRow",
        tape_width: px = "layout.tapeWidth",
        tape_col_header: px = "layout.tapeColHeader",
        tape_footer: px = "layout.tapeFooter",
        axis_width: px = "layout.axisWidth",
        time_axis: px = "layout.timeAxis",
        chart_header: px = "layout.chartHeader",
        tool_rail: px = "layout.toolRail",
        tool_button: px = "layout.toolButton",
    }
);

/// 一個主題（深色或淺色）解析完的全部 token
#[derive(Clone, Debug)]
pub struct Theme {
    pub mode: &'static str,
    pub ui: Ui,
    pub market: Market,
    pub chart: Chart,
    pub toolbar: Toolbar,
    pub drawing: Drawing,
    pub font: Font,
    pub space: Space,
    pub radius: Radius,
    pub stroke: Stroke,
    pub layout: Layout,
}

impl Theme {
    /// 從解析好的 token 表建 Theme；缺 token／型別不對會一次列出全部錯誤
    pub fn from_tokens(mode: &'static str, m: &BTreeMap<String, Tok>) -> Result<Theme, Vec<String>> {
        let mut c = Ctx { m, errs: Vec::new() };
        let t = Theme {
            mode,
            ui: Ui::load(&mut c),
            market: Market::load(&mut c),
            chart: Chart::load(&mut c),
            toolbar: Toolbar::load(&mut c),
            drawing: Drawing::load(&mut c),
            font: Font::load(&mut c),
            space: Space::load(&mut c),
            radius: Radius::load(&mut c),
            stroke: Stroke::load(&mut c),
            layout: Layout::load(&mut c),
        };
        if c.errs.is_empty() { Ok(t) } else { Err(c.errs) }
    }
}

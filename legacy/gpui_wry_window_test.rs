//! XQ-like main window, three native GPUI regions:
//!   top:          chart
//!   bottom-left:  quote list
//!   bottom-right: note only (not a webview)
//!
//! The page is a real wry webview (`gpui-wry` 0.7) in a separate window.
//! `WebViewElement::prepaint` calls `set_bounds` to the element's rectangle, but
//! `paint` only applies GPUI's `ContentMask` to GPUI drawing and mouse hits.
//! That mask does not clip the native webview. The view is a child HWND / NSView
//! painted above GPUI, and the crate README says to use a separate window or a
//! popup. This demo does not put the webview in the bottom-right pane.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    div, prelude::*, px, rgb, size, App, Bounds, Context, Entity, MouseButton, Window,
    WindowBounds, WindowOptions,
};
use gpui_wry::WebView;
struct Quote {
    symbol: &'static str,
    name: &'static str,
    price: &'static str,
    change: &'static str,
}

struct Board {
    quotes: Vec<Quote>,
    selected: usize,
    webview: Entity<WebView>,
}

/// Root of the second window. The webview fills this window; it is not a pane
/// of the main window.
struct WebHost {
    webview: Entity<WebView>,
}

impl Render for WebHost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(rgb(0x101010)).child(self.webview.clone())
    }
}

impl Render for Board {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.selected;
        let quote = &self.quotes[selected];
        let symbol = quote.symbol;
        let name = quote.name;
        let price = quote.price;
        let change = quote.change;
        let up = !change.starts_with('-');

        let mut rows = Vec::new();
        for (index, quote) in self.quotes.iter().enumerate() {
            let background = if index == selected {
                rgb(0x243044)
            } else {
                rgb(0x161616)
            };
            let color = if quote.change.starts_with('-') {
                rgb(0xe06c75)
            } else {
                rgb(0x98c379)
            };
            rows.push(
                div()
                    .id(("quote", index))
                    .flex()
                    .flex_row()
                    .justify_between()
                    .items_center()
                    .px_3()
                    .h(px(32.0))
                    .bg(background)
                    .hover(|style| style.bg(rgb(0x2c2c2c)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _event, _window, cx| {
                            this.selected = index;
                            let html = page_html(&this.quotes[index]);
                            this.webview.update(cx, |view, _| {
                                let _ = view.load_html(&html);
                            });
                            cx.notify();
                        }),
                    )
                    .child(quote.symbol)
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_3()
                            .child(quote.price)
                            .child(div().text_color(color).child(quote.change)),
                    ),
            );
        }

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x111111))
            .text_color(rgb(0xe6e6e6))
            .text_sm()
            .child(chart_pane(symbol, name, price, change, up))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .h(px(280.0))
                    .w_full()
                    .border_t_1()
                    .border_color(rgb(0x333333))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .w(px(300.0))
                            .h_full()
                            .bg(rgb(0x161616))
                            .border_r_1()
                            .border_color(rgb(0x333333))
                            .child(pane_title("Quotes"))
                            .child(
                                div()
                                    .id("watch-list")
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .overflow_y_scroll()
                                    .children(rows),
                            ),
                    )
                    .child(web_placeholder(symbol)),
            )
    }
}

fn pane_title(text: &'static str) -> impl IntoElement {
    div()
        .px_3()
        .h(px(36.0))
        .flex()
        .items_center()
        .bg(rgb(0x1c1c1c))
        .border_b_1()
        .border_color(rgb(0x333333))
        .child(text)
}

fn chart_pane(
    symbol: &'static str,
    name: &'static str,
    price: &'static str,
    change: &'static str,
    up: bool,
) -> impl IntoElement {
    let change_color = if up { rgb(0x98c379) } else { rgb(0xe06c75) };
    let mut bars = Vec::new();
    for height in series_for(symbol) {
        bars.push(
            div().flex().flex_col().justify_end().flex_1().h_full().child(
                div()
                    .w_full()
                    .h(px(height))
                    .bg(change_color)
                    .opacity(0.85),
            ),
        );
    }

    div()
        .flex()
        .flex_col()
        .flex_1()
        .w_full()
        .bg(rgb(0x101010))
        .child(pane_title("Chart"))
        .child(
            div()
                .px_4()
                .h(px(28.0))
                .flex()
                .items_center()
                .gap_3()
                .child(symbol)
                .child(name)
                .child(price)
                .child(div().text_color(change_color).child(change)),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_end()
                .flex_1()
                .gap_1()
                .px_4()
                .pb_3()
                .children(bars),
        )
}

fn web_placeholder(symbol: &'static str) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .h_full()
        .bg(rgb(0x101010))
        .child(pane_title("Web"))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .p_4()
                .child(symbol)
                .child(
                    "The page is a separate window. gpui-wry paints a native webview above GPUI; ContentMask does not clip it into this pane.",
                ),
        )
}

fn series_for(symbol: &str) -> [f32; 16] {
    let mut state = 0u32;
    for byte in symbol.bytes() {
        state = state.wrapping_mul(31).wrapping_add(byte as u32);
    }
    let mut bars = [0.0; 16];
    for bar in &mut bars {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        *bar = 24.0 + (state % 140) as f32;
    }
    bars
}

fn page_html(quote: &Quote) -> String {
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>{symbol}</title>
<style>
  body {{ margin: 0; background: #101010; color: #e6e6e6; font-family: sans-serif; }}
  main {{ padding: 28px; }}
  h1 {{ margin: 0 0 8px; font-size: 32px; }}
  p {{ margin: 0; font-size: 18px; }}
</style>
</head>
<body>
<main>
  <h1>{symbol} {name}</h1>
  <p>{price} {change}</p>
</main>
</body>
</html>"#,
        symbol = quote.symbol,
        name = quote.name,
        price = quote.price,
        change = quote.change,
    )
}

fn sample_quotes() -> Vec<Quote> {
    vec![
        Quote { symbol: "2330", name: "TSMC", price: "1,035", change: "+12.00" },
        Quote { symbol: "2317", name: "Hon Hai", price: "198.5", change: "-1.50" },
        Quote { symbol: "2454", name: "MediaTek", price: "1,280", change: "+8.00" },
        Quote { symbol: "0050", name: "Yuanta 50", price: "186.2", change: "+0.40" },
    ]
}

fn open_webview_window(html: String, cx: &mut App) -> Entity<WebView> {
    let slot: Rc<RefCell<Option<Entity<WebView>>>> = Rc::new(RefCell::new(None));
    let slot_in_window = slot.clone();
    let bounds = Bounds::centered(None, size(px(640.0), px(480.0)), cx);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        move |window, cx| {
            // Window::window_handle() is GPUI's AnyWindowHandle. The raw handle
            // is the HasWindowHandle impl, and build_as_child takes that directly.
            let child = wry::WebViewBuilder::new()
                .with_html(html)
                .build_as_child(window)
                .expect("build child webview");
            let webview = cx.new(|cx| WebView::new(child, window, cx));
            *slot_in_window.borrow_mut() = Some(webview.clone());
            cx.new(|_| WebHost { webview })
        },
    )
    .expect("open webview window");
    let webview = slot.borrow().clone().expect("webview entity");
    webview
}

fn main() {
    // WebView2 and GPUI both want DirectComposition. gpui-wry's example
    // requires this before the app starts, or the child webview stays blank.
    #[cfg(target_os = "windows")]
    unsafe {
        std::env::set_var("GPUI_DISABLE_DIRECT_COMPOSITION", "true");
    }

    gpui_platform::application().run(|cx: &mut App| {
        let quotes = sample_quotes();
        let html = page_html(&quotes[0]);
        let webview = open_webview_window(html, cx);

        let bounds = Bounds::centered(None, size(px(1100.0), px(720.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            move |_window, cx| {
                cx.new(|_| Board {
                    quotes,
                    selected: 0,
                    webview,
                })
            },
        )
        .expect("open main window");
        cx.activate(true);
    });
}

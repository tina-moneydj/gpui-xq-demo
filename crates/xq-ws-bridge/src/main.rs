//! xq-ws-bridge：瀏覽器不能開 TCP，所以網頁版（egui／GPUI 的 wasm build）透過這支小程式連行情。
//!
//! 每一條 WebSocket 連線 = 對行情來源開一條 TCP 連線，兩邊的位元組原封不動轉送：
//! - TCP → WS：每次 read() 讀到的位元組包成一個 binary message（可能是半包或多包，瀏覽器端用
//!   xq-feed 的 `FrameReader` 切包、`decode` 解碼——跟原生版同一份程式碼）。
//! - WS → TCP：瀏覽器送來的訂閱封包（`encode_subscribe` 的輸出）直接寫進 TCP。
//! 不解碼、不合併、不節流，所以壓測量到的差異來自瀏覽器端，不是 bridge。
//!
//! 另外順便當靜態檔案伺服器（`--www <dir>`）：HTTP GET 會回 dir 底下的檔案（給 wasm／html／字型用），
//! 同一個 port 上 `Upgrade: websocket` 的請求才走行情轉送。
//!
//! 用法：xq-ws-bridge [--listen 127.0.0.1:47632] [--feed 127.0.0.1:47631] [--www dist]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;

static CONN_ID: AtomicU64 = AtomicU64::new(0);

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let mut listen: SocketAddr = ([127, 0, 0, 1], 47632).into();
    let mut feed: SocketAddr = ([127, 0, 0, 1], 47631).into();
    let mut www: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--listen" => listen = args.next().and_then(|v| v.parse().ok()).expect("--listen host:port"),
            "--feed" => feed = args.next().and_then(|v| v.parse().ok()).expect("--feed host:port"),
            "--www" => www = args.next().map(PathBuf::from),
            _ => {
                eprintln!("用法：xq-ws-bridge [--listen 127.0.0.1:47632] [--feed 127.0.0.1:47631] [--www dir]");
                std::process::exit(2);
            }
        }
    }
    let www = Arc::new(www);
    let l = TcpListener::bind(listen).await.expect("bind");
    eprintln!("[xq-ws-bridge] ws://{listen} → tcp://{feed}  www={:?}", www.as_ref());
    loop {
        let Ok((sock, peer)) = l.accept().await else { continue };
        let www = www.clone();
        tokio::spawn(async move {
            let _ = sock.set_nodelay(true);
            // 先偷看請求頭：有 Upgrade: websocket 才走 WS，否則當靜態檔案
            let mut head = [0u8; 2048];
            let n = match sock.peek(&mut head).await { Ok(n) => n, Err(_) => return };
            let req = String::from_utf8_lossy(&head[..n]).to_ascii_lowercase();
            if req.contains("upgrade: websocket") {
                relay(sock, peer, feed).await;
            } else {
                serve_file(sock, www.as_ref().as_ref()).await;
            }
        });
    }
}

async fn relay(sock: TcpStream, peer: SocketAddr, feed: SocketAddr) {
    let id = CONN_ID.fetch_add(1, Ordering::Relaxed);
    let ws = match tokio_tungstenite::accept_async(sock).await {
        Ok(ws) => ws,
        Err(e) => return eprintln!("[xq-ws-bridge #{id}] handshake {peer}: {e}"),
    };
    let tcp = match TcpStream::connect(feed).await {
        Ok(t) => t,
        Err(e) => return eprintln!("[xq-ws-bridge #{id}] 連不上行情 {feed}: {e}"),
    };
    let _ = tcp.set_nodelay(true);
    eprintln!("[xq-ws-bridge #{id}] {peer} ↔ {feed}");
    let (mut tr, mut tw) = tcp.into_split();
    let (mut wsw, mut wsr) = ws.split();
    let up = async move {
        let mut buf = vec![0u8; 256 * 1024];
        let mut bytes = 0u64;
        loop {
            match tr.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    bytes += n as u64;
                    if wsw.send(Message::Binary(buf[..n].to_vec().into())).await.is_err() {
                        break;
                    }
                }
            }
        }
        let _ = wsw.close().await;
        bytes
    };
    let down = async move {
        while let Some(Ok(m)) = wsr.next().await {
            match m {
                Message::Binary(b) => {
                    if tw.write_all(&b).await.is_err() {
                        break;
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
        let _ = tw.shutdown().await;
    };
    tokio::select! {
        b = up => eprintln!("[xq-ws-bridge #{id}] 行情端關閉（轉送 {} KB）", b / 1024),
        _ = down => eprintln!("[xq-ws-bridge #{id}] 瀏覽器端關閉"),
    }
}

async fn serve_file(mut sock: TcpStream, www: Option<&PathBuf>) {
    let mut buf = vec![0u8; 8192];
    let n = sock.read(&mut buf).await.unwrap_or(0);
    let req = String::from_utf8_lossy(&buf[..n]);
    let path = req.split_whitespace().nth(1).unwrap_or("/").split('?').next().unwrap_or("/").to_string();
    let rel = if path == "/" { "index.html".to_string() } else { path.trim_start_matches('/').to_string() };
    let body = match www {
        Some(dir) if !rel.contains("..") => tokio::fs::read(dir.join(&rel)).await.ok(),
        _ => None,
    };
    let Some(body) = body else {
        let _ = sock.write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n").await;
        return;
    };
    let ct = match rel.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript",
        "wasm" => "application/wasm",
        "json" => "application/json",
        "ttf" | "otf" => "font/ttf",
        "css" => "text/css",
        _ => "application/octet-stream",
    };
    // COOP/COEP：讓 SharedArrayBuffer（wasm 多執行緒）可用；no-store：每次量載入都是冷的
    let head = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: {ct}\r\ncontent-length: {}\r\ncache-control: no-store\r\n\
         cross-origin-opener-policy: same-origin\r\ncross-origin-embedder-policy: require-corp\r\nconnection: close\r\n\r\n",
        body.len()
    );
    let _ = sock.write_all(head.as_bytes()).await;
    let _ = sock.write_all(&body).await;
}

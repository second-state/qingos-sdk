//! app 在局域网上等别人连进来：系统开端口、收连接，按行交给 app。app 不碰 socket。
//!
//! - 只收局域网来的：私有地址（10/8、172.16/12、192.168/16）、链路本地（169.254/16）和本机（127/8）。别的直接关。
//! - 同时最多 [`MAX_CONNS`] 个连接，多的直接关。
//! - 一行最长 [`MAX_LINE`] 字节（不算换行），再长的整行丢掉，连接不断。
//! - 事件：`{kind: "conn", id, what: "open", peer}`、`{..., what: "line", text}`、`{..., what: "close"}`。
//! - 日志：开端口、撤端口、每个连接来和走，各一行。收发的内容不记。
//!
//! 设备上的运行时和开发机上的 [`crate::Player`] 都用这一份。mDNS 不在这里：设备上运行时回，开发机上只记日志。

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 同时最多几个连接。
pub const MAX_CONNS: usize = 4;
/// 一行最长几个字节，不算换行。
pub const MAX_LINE: usize = 8192;
/// 发一行最多等几秒。对方不收，到点就断掉它。
const WRITE_SECS: u64 = 5;

/// 交事件的函数：收一个 `{kind: "conn", ...}`。
pub type Deliver = Arc<dyn Fn(Value) + Send + Sync>;

/// 这个地址能不能连进来：只收局域网和本机。本机上的程序本来就能从局域网的地址连进来，拦本机没有用。
pub fn allowed(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v.is_private() || v.is_link_local() || v.is_loopback(),
        IpAddr::V6(v) => v.to_ipv4_mapped().is_some_and(|v| allowed(IpAddr::V4(v))),
    }
}

/// 把收到的字节切成行。超长的一行整行丢掉，到下一个换行再接着收。行尾的 `\r` 去掉。
#[derive(Default)]
pub struct Lines {
    cur: Vec<u8>,
    skip: bool,
}

impl Lines {
    /// 交一段字节，回切出来的整行和丢了几行。
    pub fn push(&mut self, bytes: &[u8]) -> (Vec<String>, usize) {
        let (mut out, mut dropped) = (Vec::new(), 0);
        for &b in bytes {
            if b == b'\n' {
                if self.skip {
                    dropped += 1;
                } else {
                    if self.cur.last() == Some(&b'\r') {
                        self.cur.pop();
                    }
                    out.push(String::from_utf8_lossy(&self.cur).into_owned());
                }
                self.cur.clear();
                self.skip = false;
            } else if !self.skip {
                if self.cur.len() == MAX_LINE {
                    self.skip = true;
                    self.cur.clear();
                } else {
                    self.cur.push(b);
                }
            }
        }
        (out, dropped)
    }
}

/// `mdns_set` 的参数：`{instance, txt: {k: v}}`。实例名不能空、不能有点，最长 63 字节；每项 `k=v` 最长 255 字节，k 不能空。
/// 不对回 None。
pub fn parse_mdns_set(bytes: &[u8]) -> Option<(String, Vec<(String, String)>)> {
    let v: Value = serde_json::from_slice(bytes).ok()?;
    let instance = v["instance"].as_str()?.to_string();
    if instance.is_empty() || instance.len() > 63 || instance.contains('.') {
        return None;
    }
    let mut txt = Vec::new();
    if let Some(m) = v["txt"].as_object() {
        for (k, val) in m {
            let val = val.as_str()?;
            if k.is_empty() || k.contains('=') || k.len() + 1 + val.len() > 255 {
                return None;
            }
            txt.push((k.clone(), val.to_string()));
        }
    }
    Some((instance, txt))
}

/// `n` 个随机字节（最多 4096），从系统的随机源来。读不了回空的。
pub fn random(n: usize) -> Vec<u8> {
    let mut b = vec![0u8; n.min(4096)];
    match std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)) {
        Ok(()) => b,
        Err(_) => Vec::new(),
    }
}

/// 一个 app 的端口：开、撤、往连接发一行、关一条连接。撤了还能再开（整屏 app 停下又接着跑）。
pub struct Listener {
    app: String,
    port: u16,
    deliver: Deliver,
    conns: Arc<Mutex<HashMap<u64, TcpStream>>>,
    next: Arc<AtomicU64>,
    running: Mutex<Option<Running>>,
}

struct Running {
    stop: Arc<AtomicBool>,
    thread: std::thread::JoinHandle<()>,
    bound: u16,
}

impl Listener {
    /// `port` 是 0 时由系统挑一个（测试用），开了以后看 [`Listener::bound`]。
    pub fn new(app: &str, port: u16, deliver: Deliver) -> Listener {
        Listener { app: app.to_string(), port, deliver, conns: Arc::default(), next: Arc::default(), running: Mutex::new(None) }
    }

    /// 开端口。已经开着就不动。回开在哪个端口上。
    pub fn start(&self) -> std::io::Result<u16> {
        let mut g = self.running.lock().unwrap();
        if let Some(r) = g.as_ref() {
            return Ok(r.bound);
        }
        let l = TcpListener::bind(("0.0.0.0", self.port))?;
        l.set_nonblocking(true)?;
        let bound = l.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let (app, conns, next, deliver, stop2) = (self.app.clone(), self.conns.clone(), self.next.clone(), self.deliver.clone(), stop.clone());
        let thread = std::thread::Builder::new().name(format!("listen-{}", self.app)).spawn(move || {
            while !stop2.load(Ordering::Relaxed) {
                match l.accept() {
                    Ok((s, peer)) => accepted(&app, bound, s, peer, &conns, &next, &deliver),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(50)),
                    Err(_) => std::thread::sleep(Duration::from_millis(200)),
                }
            }
        })?;
        eprintln!("[listen] {} {bound} 开了", self.app);
        *g = Some(Running { stop, thread, bound });
        Ok(bound)
    }

    /// 撤端口：不再收新的，关掉所有连接（每条来一个 close）。没开着就不动。
    pub fn stop(&self) {
        let Some(r) = self.running.lock().unwrap().take() else { return };
        r.stop.store(true, Ordering::Relaxed);
        let _ = r.thread.join();
        for (_, s) in self.conns.lock().unwrap().drain() {
            let _ = s.shutdown(Shutdown::Both);
        }
        eprintln!("[listen] {} {} 撤了", self.app, r.bound);
    }

    /// 开在哪个端口上，没开是 None。
    pub fn bound(&self) -> Option<u16> {
        self.running.lock().unwrap().as_ref().map(|r| r.bound)
    }

    /// 现在有几个连接。
    pub fn conns(&self) -> usize {
        self.conns.lock().unwrap().len()
    }

    /// 往一条连接发一行，补上换行。连接不在、太长、行里有换行、发不出去回 false。发不出去的连接关掉。
    pub fn send(&self, id: u64, line: &str) -> bool {
        if line.len() > MAX_LINE || line.contains('\n') {
            return false;
        }
        let Some(mut s) = self.conns.lock().unwrap().get(&id).and_then(|s| s.try_clone().ok()) else { return false };
        let mut b = Vec::with_capacity(line.len() + 1);
        b.extend_from_slice(line.as_bytes());
        b.push(b'\n');
        if s.write_all(&b).is_ok() {
            return true;
        }
        let _ = s.shutdown(Shutdown::Both);
        false
    }

    /// 关掉一条连接。之后来一个 close。
    pub fn close(&self, id: u64) {
        if let Some(s) = self.conns.lock().unwrap().remove(&id) {
            let _ = s.shutdown(Shutdown::Both);
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop();
    }
}

fn accepted(app: &str, port: u16, s: TcpStream, peer: std::net::SocketAddr, conns: &Arc<Mutex<HashMap<u64, TcpStream>>>, next: &AtomicU64, deliver: &Deliver) {
    let ip = peer.ip();
    if !allowed(ip) {
        eprintln!("[listen] {app} {port} 不是局域网的 {ip}，关掉");
        let _ = s.shutdown(Shutdown::Both);
        return;
    }
    // Mac 上收进来的 socket 跟着监听的那个是非阻塞的，Linux 上不是：都设成阻塞。
    let _ = s.set_nonblocking(false);
    let _ = s.set_write_timeout(Some(Duration::from_secs(WRITE_SECS)));
    let Ok(writer) = s.try_clone() else { return };
    let id = {
        let mut m = conns.lock().unwrap();
        if m.len() >= MAX_CONNS {
            drop(m);
            eprintln!("[listen] {app} {port} 满了（{MAX_CONNS} 个），关掉 {ip}");
            let _ = s.shutdown(Shutdown::Both);
            return;
        }
        let id = next.fetch_add(1, Ordering::Relaxed) + 1;
        m.insert(id, writer);
        id
    };
    eprintln!("[listen] {app} {port} 来了 {ip}");
    deliver(json!({ "kind": "conn", "id": id, "what": "open", "peer": peer.to_string() }));
    let (app, conns2, deliver) = (app.to_string(), conns.clone(), deliver.clone());
    let spawned = std::thread::Builder::new().name(format!("conn-{app}-{id}")).spawn(move || {
        let mut s = s;
        let (mut lines, mut buf, mut said) = (Lines::default(), [0u8; 4096], false);
        loop {
            match s.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let (got, dropped) = lines.push(&buf[..n]);
                    if dropped > 0 && !std::mem::replace(&mut said, true) {
                        eprintln!("[listen] {app} {port} {ip} 发来的一行超过 {MAX_LINE} 字节，丢掉");
                    }
                    for text in got {
                        deliver(json!({ "kind": "conn", "id": id, "what": "line", "text": text }));
                    }
                }
            }
        }
        conns2.lock().unwrap().remove(&id);
        eprintln!("[listen] {app} {port} 走了 {ip}");
        deliver(json!({ "kind": "conn", "id": id, "what": "close" }));
    });
    if spawned.is_err() {
        if let Some(s) = conns.lock().unwrap().remove(&id) {
            let _ = s.shutdown(Shutdown::Both);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::Ipv4Addr;
    use std::sync::mpsc;

    fn listener() -> (Listener, mpsc::Receiver<Value>, u16) {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let l = Listener::new("t", 0, Arc::new(move |v| tx.lock().unwrap().send(v).unwrap()));
        let port = l.start().unwrap();
        (l, rx, port)
    }

    fn next(rx: &mpsc::Receiver<Value>) -> Value {
        rx.recv_timeout(Duration::from_secs(3)).expect("没等到事件")
    }

    #[test]
    fn only_lan_and_this_machine_get_in() {
        for ok in ["10.1.2.3", "172.16.0.1", "172.31.255.1", "192.168.1.20", "169.254.3.4", "127.0.0.1"] {
            assert!(allowed(ok.parse().unwrap()), "{ok}");
        }
        for no in ["8.8.8.8", "172.32.0.1", "100.64.0.1", "2001:db8::1"] {
            assert!(!allowed(no.parse().unwrap()), "{no}");
        }
        assert!(allowed(IpAddr::V6(Ipv4Addr::new(192, 168, 1, 2).to_ipv6_mapped())));
    }

    #[test]
    fn long_lines_are_dropped_whole_and_the_next_one_comes() {
        let mut l = Lines::default();
        let (got, d) = l.push(b"a\r\nb");
        assert_eq!((got, d), (vec!["a".to_string()], 0));
        let (got, _) = l.push(b"c\n");
        assert_eq!(got, vec!["bc"]);
        let exact = vec![b'x'; MAX_LINE];
        let (got, _) = l.push(&[&exact[..], b"\n"].concat());
        assert_eq!(got[0].len(), MAX_LINE, "正好 8192 字节的收");
        let long = vec![b'y'; MAX_LINE + 1];
        let (got, d) = l.push(&[&long[..], b"\nok\n"].concat());
        assert_eq!((got, d), (vec!["ok".to_string()], 1), "超长的整行丢掉，下一行照收");
    }

    #[test]
    fn mdns_set_checks_instance_and_txt() {
        let (i, t) = parse_mdns_set(r#"{"instance":"小黄的掌机","txt":{"key":"abc","v":"1"}}"#.as_bytes()).unwrap();
        assert_eq!(i, "小黄的掌机");
        assert_eq!(t, vec![("key".to_string(), "abc".to_string()), ("v".to_string(), "1".to_string())]);
        assert!(parse_mdns_set(br#"{"instance":"a.b"}"#).is_none());
        assert!(parse_mdns_set(br#"{"instance":""}"#).is_none());
        assert!(parse_mdns_set(format!(r#"{{"instance":"{}"}}"#, "x".repeat(64)).as_bytes()).is_none());
        assert!(parse_mdns_set(format!(r#"{{"instance":"a","txt":{{"k":"{}"}}}}"#, "x".repeat(254)).as_bytes()).is_none());
        assert_eq!(random(32).len(), 32);
    }

    #[test]
    fn a_line_in_a_line_out_and_stop_closes_everything() {
        let (l, rx, port) = listener();
        let c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let open = next(&rx);
        assert_eq!(open["what"], "open");
        let id = open["id"].as_u64().unwrap();
        (&c).write_all(b"hello\n").unwrap();
        assert_eq!(next(&rx), json!({ "kind": "conn", "id": id, "what": "line", "text": "hello" }));
        assert!(l.send(id, "back"));
        assert!(!l.send(id, "a\nb"), "行里不能有换行");
        assert!(!l.send(id, &"x".repeat(MAX_LINE + 1)), "太长");
        assert!(!l.send(id + 100, "x"), "没有这条");
        let mut r = BufReader::new(c.try_clone().unwrap());
        let mut got = String::new();
        r.read_line(&mut got).unwrap();
        assert_eq!(got, "back\n");
        l.stop();
        assert_eq!(next(&rx)["what"], "close");
        assert!(TcpStream::connect(("127.0.0.1", port)).is_err(), "撤了就连不上");
        assert!(l.start().unwrap() > 0, "撤了还能再开");
    }

    #[test]
    fn the_fifth_is_closed_and_app_can_close_one() {
        let (l, rx, port) = listener();
        let cs: Vec<TcpStream> = (0..MAX_CONNS).map(|_| TcpStream::connect(("127.0.0.1", port)).unwrap()).collect();
        let ids: Vec<u64> = (0..MAX_CONNS).map(|_| next(&rx)["id"].as_u64().unwrap()).collect();
        let fifth = TcpStream::connect(("127.0.0.1", port)).unwrap();
        fifth.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        assert_eq!((&fifth).read(&mut [0u8; 8]).unwrap_or(0), 0, "第五个直接关");
        assert_eq!(l.conns(), MAX_CONNS);
        l.close(ids[0]);
        assert_eq!(next(&rx), json!({ "kind": "conn", "id": ids[0], "what": "close" }));
        let mut c0 = &cs[0];
        c0.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        assert_eq!(c0.read(&mut [0u8; 8]).unwrap_or(0), 0, "app 关的这条，对方看到断了");
        drop(cs);
        for _ in 1..MAX_CONNS {
            assert_eq!(next(&rx)["what"], "close");
        }
        assert_eq!(l.conns(), 0);
    }
}

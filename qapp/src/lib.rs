//! 写一个 QingOS 的 app（docs/APP.md；公开的 qingos-sdk 里是 docs/ABI.md）。
//!
//! - 能力是同步的：`fn call(&self, args) -> Result<Outcome>`。wasm 里没有线程，也不用。
//! - 系统给的东西（联网、存数据、推卡、时间、日志）在 [`host`] 里，是 wasm 外的函数。
//! - 编成 `wasm32-unknown-unknown`，`qing app pack` 再把它编成本机码（`.cwasm`）装到设备上。
//!
//! ```ignore
//! use qapp::prelude::*;
//! struct Hello;
//! impl Capability for Hello {
//!     fn spec(&self) -> ToolSpec { ToolSpec::new("hello.say", "打个招呼", json!({"type":"object"})).risk(Risk::Local) }
//!     fn call(&self, _args: Value) -> Result<Outcome> {
//!         Ok(Outcome::with_card(json!({}), "打了个招呼", Effect::None, Card::text("你好", "你好。")))
//!     }
//! }
//! qapp::main!(|app| { app.add(Hello); });
//! ```
//!
//! 和运行时之间的约定（ABI）在 `docs/APP.md`「和运行时的约定」一节：模块导出 `qing_alloc / qing_free / qing_call / qinit`，
//! 导入 `qing.log / push / data_read / data_write / data_list / data_delete / now / tz_offset / http / http_start / setting / name / after`。
//! 整屏页的 `frame / buttons / tone / tone_wav` 在 [`full`] 里。
//! 来回都是 JSON 字节。

pub mod full;
pub mod lunar;
pub mod spoken;

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::sync::Mutex;

pub use qtypes::page::{Gap, Size, Tone};
pub use qtypes::{Button, Card, CardBody, Effect, Intent, ListItem, Node, Outcome, Page, Place, Principal, Risk, SettingSpec, ToolSpec};

pub mod prelude {
    pub use crate::host;
    pub use crate::spoken;
    pub use crate::{App, CallState, Capability, Event};
    pub use anyhow::{anyhow, bail, Context, Result};
    pub use qtypes::page::{Gap, Icon, Node, Page, Size, Tone};
    pub use qtypes::{Card, CardBody, Effect, Intent, ListItem, Outcome, Place, Risk, SettingSpec, ToolSpec};
    pub use serde_json::{json, Value};
}

/// 一个能力：声明加一个同步的 call。
pub trait Capability: Send {
    fn spec(&self) -> ToolSpec;
    fn call(&self, args: Value) -> Result<Outcome>;
    /// 现在能不能做。要人确认的能力，系统问人之前先调它：做不了就返回为什么，人不用白按确认键。默认能做。
    fn check(&self, _args: &Value) -> Result<()> {
        Ok(())
    }}

/// 系统转来的一件事。`App::on` 收这个；`App::on_event` 收原始的 JSON。
#[derive(Debug, Clone)]
pub enum Event {
    /// 每分钟一次（清单里订了 tick 才有）。`at` 是本地时间，RFC 3339。
    Tick { at: String },
    /// 人点了你卡上的按钮（`key`），或清单上的一条（`key` 是 `item`，`item` 是那条的 key）。`card` 是那张卡的 ref。
    Action { key: String, item: Option<String>, card: Option<String> },
    /// `host::after` 到点了。
    Timer { tag: String },
    /// `host::http_start` 做完了。`tag` 是你发请求时给的。
    Http { id: u64, tag: String, result: Result<host::Response, String> },
    /// `host::record_start` 录的那段完了：成功是整段 WAV 的字节（16 kHz、单声道、16 位），不成是为什么（包括「不发了」）。
    Recorded { id: u64, tag: String, result: Result<Vec<u8>, String> },
    /// 通话：`host::call_start` 打的那通，或 `host::call_listen` 那条连接（`id` 是 0）上来的。
    Call { id: u64, state: CallState },
    /// 有人走近（true）/ 走开。
    Near { on: bool },
    /// 静音开关。
    Mute { on: bool },
    /// 外设报的事。
    Notice { source: String, text: String },
    /// 人要全部停下（清单要了 mcu 的 app 不用订阅也收到）。要动的东西马上停。
    Halt,
    /// 人在用摇杆开你（清单写了 `stick`）：左 x、左 y、右 x、右 y，-1 到 1，右和上是正。
    /// 一秒十次左右。四个都是 0 是松开了。等不到下一个就当松开了：手机可能断了网。
    Stick { axes: [f32; 4] },
    /// 不认识的，原样给你。
    Other(Value),
}

impl Event {
    pub fn parse(v: &Value) -> Event {
        let s = |k: &str| v[k].as_str().map(str::to_string);
        match v["kind"].as_str() {
            Some("tick") => Event::Tick { at: s("at").unwrap_or_default() },
            Some("action") => Event::Action { key: s("key").unwrap_or_default(), item: s("item"), card: s("card") },
            Some("timer") => Event::Timer { tag: s("tag").unwrap_or_default() },
            Some("stick") => {
                let a = |i: usize| (v["axes"][i].as_f64().unwrap_or(0.0) as f32).clamp(-1.0, 1.0);
                Event::Stick { axes: [a(0), a(1), a(2), a(3)] }
            }
            Some("http") => Event::Http {
                id: v["id"].as_u64().unwrap_or(0),
                tag: s("tag").unwrap_or_default(),
                result: match s("error") {
                    Some(e) => Err(e),
                    None => Ok(host::Response {
                        status: v["status"].as_u64().unwrap_or(0) as u16,
                        body: v["body_b64"].as_str().and_then(qtypes::card::base64_decode).unwrap_or_default(),
                    }),
                },
            },
            Some("recorded") => Event::Recorded {
                id: v["id"].as_u64().unwrap_or(0),
                tag: s("tag").unwrap_or_default(),
                result: match s("error") {
                    Some(e) => Err(e),
                    None => Ok(v["wav_b64"].as_str().and_then(qtypes::card::base64_decode).unwrap_or_default()),
                },
            },
            Some("call") => Event::Call {
                id: v["id"].as_u64().unwrap_or(0),
                state: match v["state"].as_str() {
                    Some("open") => CallState::Open,
                    Some("text") => CallState::Text(s("text").unwrap_or_default()),
                    _ => CallState::End(s("why").unwrap_or_default()),
                },
            },
            Some("near") => Event::Near { on: v["on"].as_bool().unwrap_or(false) },
            Some("mute") => Event::Mute { on: v["on"].as_bool().unwrap_or(false) },
            Some("notice") => Event::Notice { source: s("source").unwrap_or_default(), text: s("text").unwrap_or_default() },
            Some("halt") => Event::Halt,
            _ => Event::Other(v.clone()),
        }
    }
}

/// 一通电话上来的事。
#[derive(Debug, Clone, PartialEq)]
pub enum CallState {
    /// 连上了。
    Open,
    /// 服务端来了一条文字（信令，你和服务端约定的）。
    Text(String),
    /// 结束了：为什么（挂了、断了、连不上……）。来电那条连接不会有这个，断了系统自己重连。
    End(String),
}

type EventFn = Box<dyn Fn(&Value) + Send>;
type TickFn = Box<dyn Fn() + Send>;

/// 一个 app：几个能力，可选一个事件处理函数、一个定频函数。
#[derive(Default)]
pub struct App {
    caps: Vec<Box<dyn Capability>>,
    on_event: Option<EventFn>,
    on_tick: Option<TickFn>,
}

impl App {
    pub fn add(&mut self, cap: impl Capability + 'static) {
        self.caps.push(Box::new(cap));
    }
    /// 系统转来的事件，原始 JSON：`{kind: tick, at}`、`{kind: action, key, item, card}`、`{kind: timer, tag}`、`{kind: http, …}`。
    pub fn on_event(&mut self, f: impl Fn(&Value) + Send + 'static) {
        self.on_event = Some(Box::new(f));
    }
    /// 同上，解好的 [`Event`]。
    pub fn on(&mut self, f: impl Fn(Event) + Send + 'static) {
        self.on_event = Some(Box::new(move |v| f(Event::parse(v))));
    }
    /// 清单写了 `rate_hz`：系统按这个频率调 `f`。一次要在一个周期里算完，不能等。
    pub fn every(&mut self, f: impl Fn() + Send + 'static) {
        self.on_tick = Some(Box::new(f));
    }
    fn get(&self, name: &str) -> Option<&dyn Capability> {
        self.caps.iter().find(|c| c.spec().name == name).map(|c| c.as_ref())
    }

    /// 一条请求进来（和 MCP 的方法名一样），一条回复出去。
    pub fn handle(&self, method: &str, params: Value) -> Result<Value> {
        match method {
            "initialize" => Ok(json!({ "protocolVersion": "2025-06-18", "capabilities": { "tools": {} }, "serverInfo": { "name": "qapp", "version": env!("CARGO_PKG_VERSION") } })),
            "notifications/initialized" | "ping" => Ok(Value::Null),
            "tools/list" => Ok(json!({ "tools": self.caps.iter().map(|c| c.spec().to_mcp_json()).collect::<Vec<_>>() })),
            "tools/call" => {
                let name = params["name"].as_str().ok_or_else(|| anyhow!("要有 name"))?;
                let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
                let cap = self.get(name).ok_or_else(|| anyhow!("没有「{name}」"))?;
                Ok(match cap.call(args) {
                    Ok(o) => json!({
                        "content": [{ "type": "text", "text": o.summary }],
                        "structuredContent": { "result": o.result, "summary": o.summary, "effect": o.effect, "card": o.card },
                        "isError": false
                    }),
                    Err(e) => json!({ "content": [{ "type": "text", "text": e.to_string() }], "isError": true }),
                })
            }
            "qing/check" => {
                let name = params["name"].as_str().ok_or_else(|| anyhow!("要有 name"))?;
                let cap = self.get(name).ok_or_else(|| anyhow!("没有「{name}」"))?;
                Ok(match cap.check(&params["args"]) {
                    Ok(()) => json!({}),
                    Err(e) => json!({ "why": e.to_string() }),
                })
            }
            "notifications/qing/event" => {
                if params["kind"] == "timer" {
                    host::timers::fired(params["tag"].as_str().unwrap_or(""));
                }
                if let Some(f) = &self.on_event {
                    f(&params);
                }
                Ok(Value::Null)
            }
            _ => Err(anyhow!("method not found: {method}")),
        }
    }
}

static APP: Mutex<Option<App>> = Mutex::new(None);

/// 给 `main!` 用：搭好 app 放进去。存着的定时按真实时间重新等。
#[doc(hidden)]
pub fn __init(build: impl FnOnce(&mut App)) {
    let mut app = App::default();
    build(&mut app);
    *APP.lock().unwrap() = Some(app);
    host::timers::re_arm();
}

/// 给运行时用：一条请求（JSON 字节）进来，一条回复出去。
#[doc(hidden)]
pub fn __call(req: &[u8]) -> Vec<u8> {
    let reply = (|| -> Result<Value> {
        let msg: Value = serde_json::from_slice(req)?;
        let method = msg["method"].as_str().ok_or_else(|| anyhow!("要有 method"))?;
        let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));
        let app = APP.lock().unwrap();
        let app = app.as_ref().ok_or_else(|| anyhow!("app 还没搭好"))?;
        app.handle(method, params)
    })();
    let out = match reply {
        Ok(v) => json!({ "result": v }),
        Err(e) => json!({ "error": e.to_string() }),
    };
    serde_json::to_vec(&out).unwrap_or_default()
}

/// 给运行时用：定频调一次。
#[doc(hidden)]
pub fn __tick() {
    if let Some(f) = APP.lock().unwrap().as_ref().and_then(|a| a.on_tick.as_ref()) {
        f();
    }
}

/// 把 app 搭起来、导出运行时要的几个函数。
#[macro_export]
macro_rules! main {
    ($build:expr) => {
        #[no_mangle]
        pub extern "C" fn qinit() {
            $crate::__init($build);
        }
        #[no_mangle]
        pub extern "C" fn qing_tick() {
            $crate::__tick();
        }
        #[no_mangle]
        pub extern "C" fn qing_alloc(len: u32) -> u32 {
            let mut v: Vec<u8> = Vec::with_capacity(len as usize);
            let p = v.as_mut_ptr();
            std::mem::forget(v);
            p as u32
        }
        #[no_mangle]
        pub extern "C" fn qing_free(ptr: u32, len: u32) {
            unsafe {
                drop(Vec::from_raw_parts(ptr as *mut u8, len as usize, len as usize));
            }
        }
        /// 请求是运行时用 `qing_alloc` 放进来的，这里接管、用完释放。
        /// 返回值：高 32 位是回复的地址，低 32 位是长度。运行时读完要 `qing_free`。
        #[no_mangle]
        pub extern "C" fn qing_call(ptr: u32, len: u32) -> u64 {
            let req = unsafe { Vec::from_raw_parts(ptr as *mut u8, len as usize, len as usize) };
            let out = $crate::__call(&req);
            drop(req);
            let n = out.len() as u32;
            let p = out.as_ptr() as u32;
            std::mem::forget(out);
            ((p as u64) << 32) | n as u64
        }
    };
}

/// wasm 外的东西：运行时给的函数。
pub mod host {
    use super::*;

    #[cfg(target_arch = "wasm32")]
    mod ffi {
        #[link(wasm_import_module = "qing")]
        extern "C" {
            pub fn log(ptr: u32, len: u32);
            pub fn push(ptr: u32, len: u32);
            pub fn sound(ptr: u32, len: u32) -> i32;
            pub fn sound_stop();
            pub fn page(ptr: u32, len: u32);
            pub fn data_read(kptr: u32, klen: u32) -> u64;
            pub fn asset_read(kptr: u32, klen: u32) -> u64;
            pub fn data_write(kptr: u32, klen: u32, vptr: u32, vlen: u32) -> i32;
            pub fn data_list() -> u64;
            pub fn data_delete(kptr: u32, klen: u32) -> i32;
            pub fn now() -> i64;
            pub fn tz_offset() -> i32;
            pub fn http(ptr: u32, len: u32) -> u64;
            pub fn http_start(ptr: u32, len: u32) -> u64;
            pub fn setting(kptr: u32, klen: u32) -> u64;
            pub fn location() -> u64;
            pub fn name() -> u64;
            pub fn after(secs: u64, tptr: u32, tlen: u32);
            pub fn record_start(tptr: u32, tlen: u32) -> u64;
            pub fn record_stop(id: u64, send: u32);
            pub fn call_start(ptr: u32, len: u32) -> u64;
            pub fn call_send(id: u64, ptr: u32, len: u32);
            pub fn call_stop(id: u64);
            pub fn call_listen(ptr: u32, len: u32) -> i32;
            pub fn mcu_send(ptr: u32, len: u32) -> i32;
            pub fn watch(ptr: u32, len: u32) -> i32;
            pub fn mcu_recv() -> u64;
            pub fn lan_send(ptr: u32, len: u32) -> i32;
            pub fn lan_recv() -> u64;
        }
    }

    /// 运行时写进来的一段字节：高 32 位地址，低 32 位长度；0 表示没有。
    #[cfg(target_arch = "wasm32")]
    fn take(packed: u64) -> Option<Vec<u8>> {
        if packed == 0 {
            return None;
        }
        let (p, n) = ((packed >> 32) as u32, (packed & 0xffff_ffff) as u32);
        Some(unsafe { Vec::from_raw_parts(p as *mut u8, n as usize, n as usize) })
    }

    /// 记一行日志（运行时打在系统日志里，前面带 app 的名字）。
    pub fn log(s: &str) {
        #[cfg(target_arch = "wasm32")]
        unsafe {
            ffi::log(s.as_ptr() as u32, s.len() as u32)
        }
        #[cfg(not(target_arch = "wasm32"))]
        eprintln!("[wasm-app] {s}");
    }

    /// 主动给人看一张卡。系统按节奏放。常驻的用 `card.pinned()`。
    pub fn push(card: Card) {
        let s = serde_json::to_vec(&card).unwrap_or_default();
        #[cfg(target_arch = "wasm32")]
        unsafe {
            ffi::push(s.as_ptr() as u32, s.len() as u32)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = s;
            eprintln!("[wasm-app] 推卡：{}", card.title().unwrap_or(""));
        }
    }

    /// 放一段短声音：WAV 的字节，最长 30 秒。不进卡流，不攒；静音、全部停下时不放。
    /// 合成是 app 的事。不是 WAV、太长的系统丢掉，回 false。
    pub fn sound(wav: &[u8]) -> bool {
        #[cfg(target_arch = "wasm32")]
        unsafe {
            ffi::sound(wav.as_ptr() as u32, wav.len() as u32) == 0
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            eprintln!("[wasm-app] 放声音：{} 字节", wav.len());
            wav.len() >= 44 && &wav[..4] == b"RIFF"
        }
    }

    /// 停掉自己放的、还没放完的那段声音（唱到一半人说别唱了）。系统说的话不归你停：你的那段放完了就不管。
    pub fn sound_stop() {
        #[cfg(target_arch = "wasm32")]
        unsafe {
            ffi::sound_stop()
        }
        #[cfg(not(target_arch = "wasm32"))]
        eprintln!("[wasm-app] 停声音");
    }

    /// 交一页：一整屏的界面树，系统画。同一个 ref 的新页顶掉旧的；
    /// 人正看着这一页就立刻换。版面上指向这个 ref 的按钮会打开它。按钮点了，app 收到 `action`，card 是这个 ref。
    pub fn page(p: Page) {
        let s = serde_json::to_vec(&p).unwrap_or_default();
        #[cfg(target_arch = "wasm32")]
        unsafe {
            ffi::page(s.as_ptr() as u32, s.len() as u32)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = s;
            native::PAGES.lock().unwrap().push(p);
        }
    }

    /// app 自己的数据：按名字存一段字节。设备上在状态盘，只有这个 app 看得到。
    /// 名字是一个文件名：没有斜杠，不以 `.` 开头。
    pub fn data_read(key: &str) -> Option<Vec<u8>> {
        #[cfg(target_arch = "wasm32")]
        {
            take(unsafe { ffi::data_read(key.as_ptr() as u32, key.len() as u32) })
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            native::DATA.lock().unwrap().get(key).cloned()
        }
    }

    /// 跟着 app 装来的文件：app 目录 assets/ 下的，只读。名字的规矩和数据一样。
    pub fn asset_read(name: &str) -> Option<Vec<u8>> {
        #[cfg(target_arch = "wasm32")]
        {
            take(unsafe { ffi::asset_read(name.as_ptr() as u32, name.len() as u32) })
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            native::ASSETS.lock().unwrap().get(name).cloned()
        }
    }

    pub fn data_write(key: &str, value: &[u8]) -> Result<()> {
        #[cfg(target_arch = "wasm32")]
        {
            let r = unsafe { ffi::data_write(key.as_ptr() as u32, key.len() as u32, value.as_ptr() as u32, value.len() as u32) };
            if r != 0 {
                bail!("存不了 {key}");
            }
            Ok(())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            native::DATA.lock().unwrap().insert(key.to_string(), value.to_vec());
            Ok(())
        }
    }

    /// 存着的所有名字，排好序。清旧照片、旧声音靠它。
    pub fn data_list() -> Vec<String> {
        #[cfg(target_arch = "wasm32")]
        {
            take(unsafe { ffi::data_list() }).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut v: Vec<String> = native::DATA.lock().unwrap().keys().cloned().collect();
            v.sort();
            v
        }
    }

    /// 删一个。本来就没有也算成功。
    pub fn data_delete(key: &str) -> Result<()> {
        #[cfg(target_arch = "wasm32")]
        {
            if unsafe { ffi::data_delete(key.as_ptr() as u32, key.len() as u32) } != 0 {
                bail!("删不了 {key}");
            }
            Ok(())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            native::DATA.lock().unwrap().remove(key);
            Ok(())
        }
    }

    /// 现在：Unix 秒，和设备的时区偏移（秒）。
    pub fn now() -> (i64, i32) {
        #[cfg(target_arch = "wasm32")]
        unsafe {
            (ffi::now(), ffi::tz_offset())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
            // 开发机上按本机时区：测试里的"今天"要和人看的一样。
            let off = chrono::Local::now().offset().local_minus_utc();
            (t, off)
        }
    }

    /// 联网回来的东西。
    #[derive(Debug, Clone)]
    pub struct Response {
        pub status: u16,
        pub body: Vec<u8>,
    }

    impl Response {
        pub fn text(&self) -> String {
            String::from_utf8_lossy(&self.body).into_owned()
        }
        pub fn ok(&self) -> bool {
            (200..300).contains(&self.status)
        }
        pub fn json(&self) -> Result<Value> {
            Ok(serde_json::from_slice(&self.body)?)
        }
    }

    fn http_request(method: &str, url: &str, headers: &[(&str, &str)], body: Option<&[u8]>, tag: Option<&str>) -> Value {
        json!({
            "method": method,
            "url": url,
            "headers": headers.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>(),
            "body_b64": body.map(qtypes::card::base64_encode),
            "tag": tag,
        })
    }

    /// 联网：运行时替你发（走底座的 DNS，二十秒超时）。清单里要了网络才有。
    /// `headers` 是几对请求头；`body` 是请求正文（字节）。回来的正文也是字节，`text()` 拿字符串。
    /// 这是同步的：等的时候你的 app 什么都收不到（人点的按钮也排在后面）。大的、多的用 [`http_start`]。
    pub fn http(method: &str, url: &str, headers: &[(&str, &str)], body: Option<&[u8]>) -> Result<Response> {
        let req = http_request(method, url, headers, body, None);
        #[cfg(target_arch = "wasm32")]
        {
            let s = serde_json::to_vec(&req)?;
            let r = take(unsafe { ffi::http(s.as_ptr() as u32, s.len() as u32) }).ok_or_else(|| anyhow!("运行时没回"))?;
            let v: Value = serde_json::from_slice(&r)?;
            if let Some(e) = v["error"].as_str() {
                bail!("{e}");
            }
            let body = v["body_b64"].as_str().and_then(qtypes::card::base64_decode).unwrap_or_default();
            Ok(Response { status: v["status"].as_u64().unwrap_or(0) as u16, body })
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            match native::HTTP.lock().unwrap().as_ref() {
                Some(f) => f(&req),
                None => bail!("开发机上直接跑的 app 不能联网（测试里用 host::test::set_http 装一个假的）"),
            }
        }
    }

    /// 异步联网：现在就回，做完了给你一个 [`Event::Http`]（带你给的 `tag`）。
    /// 返回请求的编号；0 是没发出去（清单没要网络，或运行时不支持）。
    /// 同步要拉好几样东西的 app 用这个：tick 里发出去就返回，人点按钮不用等。
    pub fn http_start(method: &str, url: &str, headers: &[(&str, &str)], body: Option<&[u8]>, tag: &str) -> u64 {
        let req = http_request(method, url, headers, body, Some(tag));
        #[cfg(target_arch = "wasm32")]
        {
            let s = serde_json::to_vec(&req).unwrap_or_default();
            unsafe { ffi::http_start(s.as_ptr() as u32, s.len() as u32) }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            // 开发机上：假的 http 立刻做完，事件排进队列，测试用 `host::test::pump` 送进 app。
            let Some(r) = native::HTTP.lock().unwrap().as_ref().map(|f| f(&req)) else { return 0 };
            let id = native::NEXT_HTTP.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            let mut ev = match r {
                Ok(r) => json!({ "status": r.status, "body_b64": qtypes::card::base64_encode(&r.body) }),
                Err(e) => json!({ "error": e.to_string() }),
            };
            ev["kind"] = json!("http");
            ev["id"] = json!(id);
            ev["tag"] = json!(tag);
            native::EVENTS.lock().unwrap().push(ev);
            id
        }
    }

    /// 开始录一段话：系统叮一声开始录，录完给你一个 [`Event::Recorded`]（带你给的 `tag`）。
    /// 最长 60 秒，到了自己停、交给你。返回这段的编号；0 是没开始：清单没要 `mic`、不是在处理人点的按钮（[`Event::Action`]）、
    /// 已经有一段在录。
    pub fn record_start(tag: &str) -> u64 {
        #[cfg(target_arch = "wasm32")]
        unsafe {
            ffi::record_start(tag.as_ptr() as u32, tag.len() as u32)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut c = native::CLIP.lock().unwrap();
            if c.is_some() {
                return 0;
            }
            let id = native::NEXT_HTTP.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            *c = Some((id, tag.to_string()));
            id
        }
    }

    /// 停下自己在录的那一段。`send` 是 true：录的交给你（[`Event::Recorded`]）；false 是「不发了」，扔掉，事件里是错。
    pub fn record_stop(id: u64, send: bool) {
        #[cfg(target_arch = "wasm32")]
        unsafe {
            ffi::record_stop(id, send as u32)
        }
        #[cfg(not(target_arch = "wasm32"))]
        native::STOPS.lock().unwrap().push((id, send));
    }

    /// 打一通电话：系统连 `url`（`ws://` 或 `wss://`，口令放 `Authorization: Bearer`），叮一声，开麦。
    /// 声音系统自己转，你只管信令：连上了、来了文字、结束了都是 [`Event::Call`]，文字用 [`call_send`] 发。
    /// `who` 是和谁打，写进日志。返回这通的编号；0 是没开始：清单没要 `call`、不是在处理人点的按钮（[`Event::Action`]）、
    /// 已经有一通在打或在录一段话。
    pub fn call_start(url: &str, token: &str, who: &str) -> u64 {
        let req = json!({ "url": url, "token": token, "who": who }).to_string();
        #[cfg(target_arch = "wasm32")]
        unsafe {
            ffi::call_start(req.as_ptr() as u32, req.len() as u32)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut c = native::CALL.lock().unwrap();
            if c.is_some() {
                return 0;
            }
            let id = native::NEXT_HTTP.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            let _ = req;
            *c = Some((id, who.to_string()));
            id
        }
    }

    /// 往一通发一条文字。`id` 是 0：发到来电那条连接（[`call_listen`]），比如响铃时人点了「不接」。
    pub fn call_send(id: u64, text: &str) {
        #[cfg(target_arch = "wasm32")]
        unsafe {
            ffi::call_send(id, text.as_ptr() as u32, text.len() as u32)
        }
        #[cfg(not(target_arch = "wasm32"))]
        native::CALL_SENT.lock().unwrap().push((id, text.to_string()));
    }

    /// 挂断自己在打的那一通。之后来一个 `End`。
    pub fn call_stop(id: u64) {
        #[cfg(target_arch = "wasm32")]
        unsafe {
            ffi::call_stop(id)
        }
        #[cfg(not(target_arch = "wasm32"))]
        native::CALL_STOPS.lock().unwrap().push(id);
    }

    /// 等来电：系统替你一直连着 `url`，只传文字，断了自己重连。来的文字是 `id` 为 0 的 [`Event::Call`]。
    /// 再调一次：地址和口令一样就不动，不一样就换。清单没要 `call` 回 false。
    pub fn call_listen(url: &str, token: &str) -> bool {
        let req = json!({ "url": url, "token": token }).to_string();
        #[cfg(target_arch = "wasm32")]
        unsafe {
            ffi::call_listen(req.as_ptr() as u32, req.len() as u32) != 0
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = req;
            *native::LISTEN.lock().unwrap() = Some(url.to_string());
            true
        }
    }

    /// GET 一个地址，要字符串。不是 2xx 就报错。
    /// 给 MCU 发一帧的内容（清单要 `mcu = true`）。内容是什么，app 和 MCU 固件自己约定，最长 1024 字节。
    /// 没要 MCU、太长、线没接上回 false。
    pub fn mcu_send(payload: &[u8]) -> bool {
        #[cfg(target_arch = "wasm32")]
        unsafe {
            ffi::mcu_send(payload.as_ptr() as u32, payload.len() as u32) == 0
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            native::MCU_SENT.lock().unwrap().push(payload.to_vec());
            true
        }
    }

    /// 交最新一份自己的现况：给开发机上的人看（控制台 `GET /api/watch?app=名字`）。
    /// 系统只留最新一份，不存盘、不进日志、不给模型。最长 16 KB；太长或者交不上回 false。
    pub fn watch(v: &Value) -> bool {
        let bytes = v.to_string();
        #[cfg(target_arch = "wasm32")]
        unsafe {
            ffi::watch(bytes.as_ptr() as u32, bytes.len() as u32) == 0
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            *native::WATCHED.lock().unwrap() = Some(v.clone());
            bytes.len() <= 16 * 1024
        }
    }

    /// 取 MCU 发来的最早一帧。没有回 None。系统最多替你留 64 帧，旧的先丢。
    pub fn mcu_recv() -> Option<Vec<u8>> {
        #[cfg(target_arch = "wasm32")]
        unsafe {
            take(ffi::mcu_recv())
        }
        #[cfg(not(target_arch = "wasm32"))]
        native::MCU_IN.lock().unwrap().pop_front()
    }

    /// 在局域网里广播一段（清单要 `lan = true`）：别的设备上同名的 app 收得到，自己收不到。
    /// 最长 1200 字节。不配对、不验来源，谁都能发：收到的别当成人说的。没要、太长、网还没起来回 false。
    pub fn lan_send(payload: &[u8]) -> bool {
        #[cfg(target_arch = "wasm32")]
        unsafe {
            ffi::lan_send(payload.as_ptr() as u32, payload.len() as u32) == 0
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            native::LAN_SENT.lock().unwrap().push(payload.to_vec());
            true
        }
    }

    /// 取局域网里收到的最早一段。没有回 None。系统最多替你留 32 段，旧的先丢。
    pub fn lan_recv() -> Option<Vec<u8>> {
        #[cfg(target_arch = "wasm32")]
        unsafe {
            take(ffi::lan_recv())
        }
        #[cfg(not(target_arch = "wasm32"))]
        native::LAN_IN.lock().unwrap().pop_front()
    }

    pub fn http_get(url: &str) -> Result<String> {
        let r = http("GET", url, &[], None)?;
        if !r.ok() {
            bail!("{url} 回了 {}", r.status);
        }
        Ok(r.text())
    }

    /// GET 一个地址，解成 JSON。
    pub fn http_json(url: &str) -> Result<Value> {
        Ok(serde_json::from_str(&http_get(url)?)?)
    }

    /// POST 一段 JSON。
    pub fn http_post_json(url: &str, body: &Value) -> Result<Response> {
        http("POST", url, &[("content-type", "application/json")], Some(body.to_string().as_bytes()))
    }

    /// 设置项（清单里声明的、控制台里填的）。没有就是 None。
    /// 只认你在某个能力的 `settings` 里声明过的 key；别的回 None。开发机上声明过的也看环境变量。
    pub fn setting(key: &str) -> Option<String> {
        #[cfg(target_arch = "wasm32")]
        {
            take(unsafe { ffi::setting(key.as_ptr() as u32, key.len() as u32) }).map(|b| String::from_utf8_lossy(&b).into_owned()).filter(|s| !s.trim().is_empty())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::env::var(key).ok().filter(|s| !s.trim().is_empty())
        }
    }

    /// 设备的所在地：清单里写了 `location = true` 才有；人还没设也是 None。
    pub fn location() -> Option<crate::Place> {
        #[cfg(target_arch = "wasm32")]
        {
            take(unsafe { ffi::location() }).and_then(|b| serde_json::from_slice(&b).ok())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            native::PLACE.lock().unwrap().clone()
        }
    }

    /// 这个 app 叫什么（清单里的 name）。一个模块装成几个 app 时靠它分。
    pub fn name() -> String {
        #[cfg(target_arch = "wasm32")]
        {
            take(unsafe { ffi::name() }).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::env::var("QING_APP_NAME").unwrap_or_else(|_| "app".into())
        }
    }

    /// 过几秒给我一个事件：`{kind: "timer", tag}`。wasm 里没有线程和定时器，要等就用这个。
    /// 存在数据里：重启之后按真实时间接着等，过了点的起来就给。同一个 tag 只留最后一个。
    pub fn after(secs: u64, tag: &str) {
        timers::remember(now().0 + secs as i64, tag);
        raw_after(secs, tag);
    }

    /// 取消一个还没到的定时。
    pub fn cancel(tag: &str) {
        timers::forget(tag);
    }

    fn raw_after(secs: u64, tag: &str) {
        #[cfg(target_arch = "wasm32")]
        unsafe {
            ffi::after(secs, tag.as_ptr() as u32, tag.len() as u32)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = (secs, tag);
        }
    }

    /// 现在的本地时间（设备的时区）。别用 `chrono::Local::now()`：wasm 里没有钟。
    pub fn local_now() -> chrono::DateTime<chrono::FixedOffset> {
        use chrono::TimeZone;
        let (t, off) = now();
        let tz = chrono::FixedOffset::east_opt(off).unwrap_or_else(|| chrono::FixedOffset::east_opt(0).unwrap());
        tz.timestamp_opt(t, 0).single().unwrap_or_else(|| tz.timestamp_opt(0, 0).unwrap())
    }

    /// 定时的账：存在数据的 `timers.qapp.json` 里。app 自己别用这个名字。
    pub(crate) mod timers {
        use super::*;

        const KEY: &str = "timers.qapp.json";

        fn load() -> Vec<(i64, String)> {
            data_read(KEY).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
        }

        fn save(v: &[(i64, String)]) {
            if v.is_empty() {
                let _ = data_delete(KEY);
            } else if let Ok(b) = serde_json::to_vec(v) {
                let _ = data_write(KEY, &b);
            }
        }

        pub fn remember(due: i64, tag: &str) {
            let mut v = load();
            v.retain(|(_, t)| t != tag);
            v.push((due, tag.to_string()));
            save(&v);
        }

        pub fn forget(tag: &str) {
            let mut v = load();
            v.retain(|(_, t)| t != tag);
            save(&v);
        }

        /// 到点了：从账上去掉。取消过的不在账上，事件照样会来，app 自己认 tag。
        pub fn fired(tag: &str) {
            forget(tag);
        }

        /// 起来的时候：账上的按真实时间重新等；过了点的马上要（0 秒）。
        pub fn re_arm() {
            let now = now().0;
            for (due, tag) in load() {
                raw_after((due - now).max(0) as u64, &tag);
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    mod native {
        use super::Response;
        use anyhow::Result;
        use serde_json::Value;
        use std::collections::HashMap;
        use std::sync::atomic::AtomicU64;
        use std::sync::{LazyLock, Mutex};
        pub type HttpFn = Box<dyn Fn(&Value) -> Result<Response> + Send>;
        pub static DATA: LazyLock<Mutex<HashMap<String, Vec<u8>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
        pub static ASSETS: LazyLock<Mutex<HashMap<String, Vec<u8>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
        pub static HTTP: Mutex<Option<HttpFn>> = Mutex::new(None);
        pub static EVENTS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
        pub static NEXT_HTTP: AtomicU64 = AtomicU64::new(0);
        pub static PAGES: Mutex<Vec<crate::Page>> = Mutex::new(Vec::new());
        pub static PLACE: Mutex<Option<crate::Place>> = Mutex::new(None);
        /// 在录的那一段：编号、记号。
        pub static CLIP: Mutex<Option<(u64, String)>> = Mutex::new(None);
        /// app 调过的 `record_stop`：编号、要不要。
        pub static STOPS: Mutex<Vec<(u64, bool)>> = Mutex::new(Vec::new());
        /// 在打的那一通：编号、和谁。
        pub static CALL: Mutex<Option<(u64, String)>> = Mutex::new(None);
        /// app 调过的 `call_send`：编号、文字。
        pub static CALL_SENT: Mutex<Vec<(u64, String)>> = Mutex::new(Vec::new());
        /// app 调过的 `call_stop`。
        pub static CALL_STOPS: Mutex<Vec<u64>> = Mutex::new(Vec::new());
        /// `call_listen` 给的地址。
        pub static LISTEN: Mutex<Option<String>> = Mutex::new(None);
        /// app 发给 MCU 的。
        pub static MCU_SENT: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
        pub static WATCHED: Mutex<Option<Value>> = Mutex::new(None);
        /// MCU 发来、app 还没取的。
        pub static MCU_IN: Mutex<std::collections::VecDeque<Vec<u8>>> = Mutex::new(std::collections::VecDeque::new());
        pub static LAN_SENT: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
        pub static LAN_IN: Mutex<std::collections::VecDeque<Vec<u8>>> = Mutex::new(std::collections::VecDeque::new());
    }

    /// 本机测试用的替身。
    #[cfg(not(target_arch = "wasm32"))]
    pub mod test {
        use super::*;

        /// 装一个假的联网：`f` 收到 `{method, url, headers, body_b64, tag}`，回一个 [`Response`] 或错。
        /// 之后 `host::http` 和 `host::http_start` 都走它。
        pub fn set_http(f: impl Fn(&Value) -> Result<Response> + Send + 'static) {
            *native::HTTP.lock().unwrap() = Some(Box::new(f));
        }

        /// 把 `http_start` 攒下的结果按顺序送进 app（走 `notifications/qing/event`）。返回送了几条。
        pub fn pump() -> usize {
            let evs: Vec<Value> = std::mem::take(&mut *native::EVENTS.lock().unwrap());
            let n = evs.len();
            for ev in evs {
                let _ = crate::__call(&serde_json::to_vec(&json!({ "method": "notifications/qing/event", "params": ev })).unwrap_or_default());
            }
            n
        }

        /// `http_start` 攒下、还没送进 app 的结果。
        pub fn pending() -> Vec<Value> {
            native::EVENTS.lock().unwrap().clone()
        }

        /// app 交过的页，按交的顺序。
        pub fn pages() -> Vec<crate::Page> {
            native::PAGES.lock().unwrap().clone()
        }

        /// 设一个所在地，之后 `host::location` 回它。
        pub fn set_place(p: Option<crate::Place>) {
            *native::PLACE.lock().unwrap() = p;
        }

        /// 在录的那一段（编号、记号），没在录是 None。
        pub fn recording() -> Option<(u64, String)> {
            native::CLIP.lock().unwrap().clone()
        }

        /// app 调过的 `record_stop`（编号、要不要），按顺序。
        pub fn stops() -> Vec<(u64, bool)> {
            native::STOPS.lock().unwrap().clone()
        }

        /// 在录的那一段完了：把结果排进队列（`pump` 送进 app）。没在录就什么都不做。
        pub fn finish_record(result: std::result::Result<Vec<u8>, String>) {
            let Some((id, tag)) = native::CLIP.lock().unwrap().take() else { return };
            let mut ev = match result {
                Ok(b) => json!({ "wav_b64": qtypes::card::base64_encode(&b) }),
                Err(e) => json!({ "error": e }),
            };
            ev["kind"] = json!("recorded");
            ev["id"] = json!(id);
            ev["tag"] = json!(tag);
            native::EVENTS.lock().unwrap().push(ev);
        }

        /// 在打的那一通（编号、和谁），没在打是 None。
        pub fn calling() -> Option<(u64, String)> {
            native::CALL.lock().unwrap().clone()
        }

        /// app 调过的 `call_send`（编号、文字），按顺序。取走。
        pub fn call_sent() -> Vec<(u64, String)> {
            std::mem::take(&mut *native::CALL_SENT.lock().unwrap())
        }

        /// app 调过的 `call_stop`，按顺序。
        pub fn call_stops() -> Vec<u64> {
            native::CALL_STOPS.lock().unwrap().clone()
        }

        /// `call_listen` 给的地址。
        pub fn listening() -> Option<String> {
            native::LISTEN.lock().unwrap().clone()
        }

        /// 一通上来一件事：排进队列（`pump` 送进 app）。`End` 时那通不再算在打。
        pub fn call_event(id: u64, state: crate::CallState) {
            let mut ev = json!({ "kind": "call", "id": id });
            match state {
                crate::CallState::Open => ev["state"] = json!("open"),
                crate::CallState::Text(t) => {
                    ev["state"] = json!("text");
                    ev["text"] = json!(t);
                }
                crate::CallState::End(w) => {
                    ev["state"] = json!("end");
                    ev["why"] = json!(w);
                    let mut c = native::CALL.lock().unwrap();
                    if c.as_ref().is_some_and(|(i, _)| *i == id) {
                        *c = None;
                    }
                }
            }
            native::EVENTS.lock().unwrap().push(ev);
        }

        /// MCU 发来一帧，之后 `host::mcu_recv` 取得到。
        pub fn mcu_arrive(payload: &[u8]) {
            native::MCU_IN.lock().unwrap().push_back(payload.to_vec());
        }

        /// 局域网里别的设备发来一段，之后 `host::lan_recv` 取得到。
        pub fn lan_arrive(payload: &[u8]) {
            native::LAN_IN.lock().unwrap().push_back(payload.to_vec());
        }

        /// app 在局域网里广播的，取走。
        pub fn lan_sent() -> Vec<Vec<u8>> {
            std::mem::take(&mut *native::LAN_SENT.lock().unwrap())
        }

        /// app 发给 MCU 的，取走。
        pub fn mcu_sent() -> Vec<Vec<u8>> {
            std::mem::take(&mut *native::MCU_SENT.lock().unwrap())
        }

        /// 定频调一次（系统按清单的 `rate_hz` 调的那个）。
        pub fn tick() {
            crate::__tick();
        }

        /// 清空内存里的数据（一个测试一个干净的开始）。
        pub fn reset() {
            native::DATA.lock().unwrap().clear();
            native::EVENTS.lock().unwrap().clear();
            native::PAGES.lock().unwrap().clear();
            *native::HTTP.lock().unwrap() = None;
            *native::PLACE.lock().unwrap() = None;
            *native::CLIP.lock().unwrap() = None;
            native::STOPS.lock().unwrap().clear();
            *native::CALL.lock().unwrap() = None;
            native::CALL_SENT.lock().unwrap().clear();
            native::CALL_STOPS.lock().unwrap().clear();
            *native::LISTEN.lock().unwrap() = None;
            native::MCU_SENT.lock().unwrap().clear();
            native::MCU_IN.lock().unwrap().clear();
            native::LAN_SENT.lock().unwrap().clear();
            native::LAN_IN.lock().unwrap().clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 数据和假联网是全局的：用它们的测试排队。
    static LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn check_answers_why_or_nothing() {
        struct Picky;
        impl Capability for Picky {
            fn spec(&self) -> ToolSpec {
                ToolSpec::new("t.picky", "挑", json!({ "type": "object" }))
            }
            fn check(&self, args: &Value) -> Result<()> {
                if args["ok"] != true {
                    bail!("现在不行");
                }
                Ok(())
            }
            fn call(&self, _args: Value) -> Result<Outcome> {
                Ok(Outcome::silent(json!({}), "挑了", Effect::None))
            }
        }
        let mut app = App::default();
        app.add(Picky);
        assert_eq!(app.handle("qing/check", json!({ "name": "t.picky", "args": {} })).unwrap(), json!({ "why": "现在不行" }));
        assert_eq!(app.handle("qing/check", json!({ "name": "t.picky", "args": { "ok": true } })).unwrap(), json!({}));
    }

    #[test]
    fn event_parses_every_kind() {
        match Event::parse(&json!({ "kind": "action", "key": "item", "item": "milk", "card": "today" })) {
            Event::Action { key, item, card } => assert_eq!((key.as_str(), item.as_deref(), card.as_deref()), ("item", Some("milk"), Some("today"))),
            e => panic!("{e:?}"),
        }
        match Event::parse(&json!({ "kind": "http", "id": 3, "tag": "plan", "status": 200, "body_b64": "aGk=" })) {
            Event::Http { id: 3, tag, result: Ok(r) } => assert_eq!((tag.as_str(), r.status, r.text().as_str()), ("plan", 200, "hi")),
            e => panic!("{e:?}"),
        }
        match Event::parse(&json!({ "kind": "http", "id": 4, "tag": "", "error": "断了" })) {
            Event::Http { result: Err(e), .. } => assert_eq!(e, "断了"),
            e => panic!("{e:?}"),
        }
        match Event::parse(&json!({ "kind": "recorded", "id": 5, "tag": "voice", "wav_b64": "UklGRg==" })) {
            Event::Recorded { id: 5, tag, result: Ok(b) } => assert_eq!((tag.as_str(), b.as_slice()), ("voice", &b"RIFF"[..])),
            e => panic!("{e:?}"),
        }
        assert!(matches!(Event::parse(&json!({ "kind": "recorded", "id": 6, "error": "不发了" })), Event::Recorded { result: Err(_), .. }));
        match Event::parse(&json!({ "kind": "call", "id": 0, "state": "text", "text": "{}" })) {
            Event::Call { id: 0, state: CallState::Text(t) } => assert_eq!(t, "{}"),
            e => panic!("{e:?}"),
        }
        assert!(matches!(Event::parse(&json!({ "kind": "call", "id": 2, "state": "end", "why": "挂了" })), Event::Call { id: 2, state: CallState::End(_) }));
        assert!(matches!(Event::parse(&json!({ "kind": "tick", "at": "x" })), Event::Tick { .. }));
        assert!(matches!(Event::parse(&json!({ "kind": "timer", "tag": "t" })), Event::Timer { .. }));
        assert!(matches!(Event::parse(&json!({ "kind": "near", "on": true })), Event::Near { on: true }));
        assert!(matches!(Event::parse(&json!({ "kind": "halt" })), Event::Halt));
        match Event::parse(&json!({ "kind": "stick", "axes": [0.5, -1.0, 3.0] })) {
            Event::Stick { axes } => assert_eq!(axes, [0.5, -1.0, 1.0, 0.0], "超出的夹到 ±1，少了的当 0"),
            e => panic!("{e:?}"),
        }
        assert!(matches!(Event::parse(&json!({ "kind": "whatever" })), Event::Other(_)));
    }

    #[test]
    fn timers_are_remembered_until_fired() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        host::test::reset();
        host::after(60, "a");
        host::after(120, "b");
        host::after(90, "a");
        let v: Vec<(i64, String)> = serde_json::from_slice(&host::data_read("timers.qapp.json").unwrap()).unwrap();
        assert_eq!(v.len(), 2, "同一个 tag 只留最后一个");
        host::timers::fired("a");
        host::cancel("b");
        assert!(host::data_read("timers.qapp.json").is_none(), "都到点或取消了，账就没了");
    }

    #[test]
    fn fake_http_feeds_http_start_events() {
        host::test::reset();
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        host::test::reset();
        host::test::set_http(|req| {
            assert_eq!(req["url"], "http://x/plan");
            Ok(host::Response { status: 200, body: b"{}".to_vec() })
        });
        assert!(host::http_start("GET", "http://x/plan", &[], None, "plan") > 0);
        let evs = host::test::pending();
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0]["tag"], "plan");
        let r = host::http("GET", "http://x/plan", &[], None).unwrap();
        assert!(r.ok());
    }
}

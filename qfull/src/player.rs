//! 只跑整屏 app 的装载器：开发机上的命令行用它在窗口里、不开窗口、截图时跑一个游戏。
//!
//! 只接整屏 app 用得到的导入函数（`docs/ABI.md`「整屏 app」那一组）：
//! 整屏页的 `frame`、`buttons`、`tone`、`tone_wav`，加 `log`、`data_*`、`asset_read`、`now`、`tz_offset`。
//! 清单写了 `listen`，照样开端口、收连接（[`crate::listen`]），加 `conn_send`、`conn_close`、`mdns_set`、`random`。
//! 端口用 `QING_LISTEN_PORT` 换，几个仿真互不串。mDNS 在开发机上只记日志，不发。连接上的事在每一帧开头交给 app。
//! 别的导入函数（联网、推卡……）调了就出错：整屏 app 不该用它们。
//! 设备上的运行时不用这里：它有完整的 app 装载器，整屏页接的是同一份 [`crate::full::link`]。

use crate::full::{self, Full};
use crate::listen::Listener;
use std::collections::VecDeque;
use std::sync::Mutex;
use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use wasmtime::{Caller, Engine, Linker, Memory, Module, Store, StoreLimits, StoreLimitsBuilder, TypedFunc};

/// 一帧最多算几秒（epoch 一秒一跳，所以是 1–2 秒）。正常一帧是毫秒级，超了多半是死循环。
const TICK_DEADLINE_SECS: u64 = 2;

struct Host {
    full: Arc<Full>,
    data: Option<PathBuf>,
    assets: Option<PathBuf>,
    limits: StoreLimits,
    /// 清单写了 `listen` 才有。
    listen: Option<Arc<Listener>>,
    /// 清单的 `mdns`，空的是没写。
    mdns: String,
    /// 开的端口（清单的 `listen`，或者 `QING_LISTEN_PORT`）。
    port: u16,
    /// app 调过 `mdns_set` 了。
    mdns_set: bool,
}

/// 交事件要的导出：内存、`qing_alloc`、`qing_free`、`qing_call`。
struct Calls {
    memory: Memory,
    alloc: TypedFunc<u32, u32>,
    free: TypedFunc<(u32, u32), ()>,
    call: TypedFunc<(u32, u32), u64>,
}

/// 清单里装载器要看的几项。
#[derive(serde::Deserialize, Default)]
struct Manifest {
    #[serde(default)]
    app: ManifestApp,
}

#[derive(serde::Deserialize, Default)]
struct ManifestApp {
    #[serde(default)]
    name: String,
    #[serde(default)]
    listen: u16,
    #[serde(default)]
    mdns: String,
}

/// 一个跑着的整屏 app。
pub struct Player {
    store: Store<Host>,
    tick: TypedFunc<(), ()>,
    full: Arc<Full>,
    /// 交事件要的几个导出（`qapp::main!` 都导出），少一个就不交。
    call: Option<Calls>,
    /// 连接上来的事，下一帧开头交给 app。
    events: Arc<Mutex<VecDeque<serde_json::Value>>>,
    listen: Option<Arc<Listener>>,
}

fn engine() -> Result<&'static Engine> {
    static ENGINE: std::sync::OnceLock<Engine> = std::sync::OnceLock::new();
    if let Some(e) = ENGINE.get() {
        return Ok(e);
    }
    let e = Engine::new(&crate::config()).context("起不来 wasm 引擎")?;
    if ENGINE.set(e).is_ok() {
        let e = ENGINE.get().unwrap();
        std::thread::Builder::new()
            .name("wasm-epoch".into())
            .spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                e.increment_epoch();
            })
            .context("起不来 epoch 线程")?;
    }
    Ok(ENGINE.get().unwrap())
}

fn mem(c: &mut Caller<'_, Host>) -> Option<Memory> {
    c.get_export("memory").and_then(|e| e.into_memory())
}

fn bytes(m: &Memory, c: &Caller<'_, Host>, p: u32, n: u32) -> Vec<u8> {
    m.data(c).get(p as usize..p as usize + n as usize).map(<[u8]>::to_vec).unwrap_or_default()
}

/// 往 app 的内存里放一段（让它自己 `qing_alloc`），回打包的地址和长度；放不进去回 0。
fn give(c: &mut Caller<'_, Host>, m: &Memory, v: &[u8]) -> u64 {
    let Some(alloc) = c.get_export("qing_alloc").and_then(|e| e.into_func()).and_then(|f| f.typed::<u32, u32>(&*c).ok()) else { return 0 };
    let Ok(p) = alloc.call(&mut *c, v.len() as u32) else { return 0 };
    if m.write(&mut *c, p as usize, v).is_err() {
        return 0;
    }
    ((p as u64) << 32) | v.len() as u64
}

/// 数据的名字只能是一个文件名：没有斜杠、没有 `..`、不以 `.` 开头。
fn file_name(key: &[u8]) -> Option<String> {
    let k = String::from_utf8(key.to_vec()).ok()?;
    (!k.is_empty() && !k.contains('/') && !k.contains("..") && !k.starts_with('.')).then_some(k)
}

fn in_dir(dir: &Option<PathBuf>, key: &[u8]) -> Option<PathBuf> {
    Some(dir.as_ref()?.join(file_name(key)?))
}

impl Player {
    /// 装上一个整屏 app：`.wasm`（要 `compile` 特性）或 `.cwasm`。`data` 是 `data_write` 存的目录，不给就不存。
    pub fn load(path: &Path, data: Option<PathBuf>, memory_mb: u64) -> Result<Player> {
        let engine = engine()?;
        let module = if path.extension().is_some_and(|e| e == "cwasm") {
            // SAFETY: `.cwasm` 是同一份配置编的；版本对不上 wasmtime 会拒绝，不会乱跑。
            unsafe { Module::deserialize_file(engine, path) }.with_context(|| format!("读不了 {}（是别的版本编的就重新编）", path.display()))?
        } else {
            #[cfg(feature = "compile")]
            {
                Module::from_file(engine, path).with_context(|| format!("编不了 {}", path.display()))?
            }
            #[cfg(not(feature = "compile"))]
            anyhow::bail!("{}：这里只认编好的 .cwasm", path.display())
        };
        let mut linker: Linker<Host> = Linker::new(engine);
        full::link(&mut linker, |h: &Host| &h.full)?;
        linker.func_wrap("qing", "log", |mut c: Caller<'_, Host>, p: u32, n: u32| {
            if let Some(m) = mem(&mut c) {
                eprintln!("[app] {}", String::from_utf8_lossy(&bytes(&m, &c, p, n)));
            }
        })?;
        linker.func_wrap("qing", "data_read", |mut c: Caller<'_, Host>, p: u32, n: u32| -> u64 {
            let Some(m) = mem(&mut c) else { return 0 };
            let Some(f) = in_dir(&c.data().data, &bytes(&m, &c, p, n)) else { return 0 };
            std::fs::read(f).map(|v| give(&mut c, &m, &v)).unwrap_or(0)
        })?;
        linker.func_wrap("qing", "asset_read", |mut c: Caller<'_, Host>, p: u32, n: u32| -> u64 {
            let Some(m) = mem(&mut c) else { return 0 };
            let Some(f) = in_dir(&c.data().assets, &bytes(&m, &c, p, n)) else { return 0 };
            std::fs::read(f).map(|v| give(&mut c, &m, &v)).unwrap_or(0)
        })?;
        linker.func_wrap("qing", "data_write", |mut c: Caller<'_, Host>, kp: u32, kn: u32, vp: u32, vn: u32| -> i32 {
            let Some(m) = mem(&mut c) else { return 1 };
            let (key, val) = (bytes(&m, &c, kp, kn), bytes(&m, &c, vp, vn));
            let Some(f) = in_dir(&c.data().data, &key) else { return 1 };
            // 先写临时文件再换名：拔电不留半个文件。
            let tmp = f.with_file_name(format!(".{}.tmp", f.file_name().unwrap().to_string_lossy()));
            let ok = std::fs::create_dir_all(f.parent().unwrap()).is_ok() && std::fs::write(&tmp, &val).is_ok() && std::fs::rename(&tmp, &f).is_ok();
            if ok {
                0
            } else {
                1
            }
        })?;
        linker.func_wrap("qing", "data_delete", |mut c: Caller<'_, Host>, p: u32, n: u32| -> i32 {
            let Some(m) = mem(&mut c) else { return 1 };
            let Some(f) = in_dir(&c.data().data, &bytes(&m, &c, p, n)) else { return 1 };
            match std::fs::remove_file(f) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => 1,
                _ => 0,
            }
        })?;
        linker.func_wrap("qing", "data_list", |mut c: Caller<'_, Host>| -> u64 {
            let Some(m) = mem(&mut c) else { return 0 };
            let mut keys: Vec<String> = c
                .data()
                .data
                .as_ref()
                .and_then(|d| std::fs::read_dir(d).ok())
                .map(|rd| rd.flatten().filter_map(|e| e.file_name().to_str().map(str::to_string)).filter(|n| !n.starts_with('.')).collect())
                .unwrap_or_default();
            keys.sort();
            let v = serde_json_lite(&keys);
            give(&mut c, &m, v.as_bytes())
        })?;
        linker.func_wrap("qing", "conn_send", |mut c: Caller<'_, Host>, id: u64, p: u32, n: u32| -> i32 {
            let Some(m) = mem(&mut c) else { return -1 };
            let line = String::from_utf8_lossy(&bytes(&m, &c, p, n)).into_owned();
            match &c.data().listen {
                Some(l) if l.send(id, &line) => 0,
                _ => -1,
            }
        })?;
        linker.func_wrap("qing", "conn_close", |c: Caller<'_, Host>, id: u64| {
            if let Some(l) = &c.data().listen {
                l.close(id);
            }
        })?;
        linker.func_wrap("qing", "mdns_set", |mut c: Caller<'_, Host>, p: u32, n: u32| -> i32 {
            let Some(m) = mem(&mut c) else { return -1 };
            let req = bytes(&m, &c, p, n);
            let h = c.data_mut();
            if h.listen.is_none() || h.mdns.is_empty() {
                return -1;
            }
            let Some((instance, txt)) = crate::listen::parse_mdns_set(&req) else { return -1 };
            h.mdns_set = true;
            eprintln!("[mdns] 会回 {}：实例「{instance}」，端口 {}，TXT {} 项（开发机上不发）", h.mdns, h.port, txt.len());
            0
        })?;
        linker.func_wrap("qing", "random", |mut c: Caller<'_, Host>, n: u32| -> u64 {
            let Some(m) = mem(&mut c) else { return 0 };
            let b = crate::listen::random(n as usize);
            give(&mut c, &m, &b)
        })?;
        linker.func_wrap("qing", "now", |_: Caller<'_, Host>| -> i64 { chrono::Local::now().timestamp() })?;
        linker.func_wrap("qing", "tz_offset", |_: Caller<'_, Host>| -> i32 { chrono::Local::now().offset().local_minus_utc() })?;
        // 别的导入函数：装得上，调了就出错。
        linker.define_unknown_imports_as_traps(&module)?;

        let full = Arc::new(Full::default());
        let dir = path.ancestors().skip(1).find(|d| d.join("app.toml").is_file());
        let assets = dir.map(|d| d.join("assets"));
        let manifest: Manifest = match dir {
            Some(d) => {
                let f = d.join("app.toml");
                let text = std::fs::read_to_string(&f).with_context(|| format!("读不了 {}", f.display()))?;
                toml::from_str(&text).with_context(|| format!("{} 格式不对", f.display()))?
            }
            None => Manifest::default(),
        };
        let events: Arc<Mutex<VecDeque<serde_json::Value>>> = Arc::default();
        let port = std::env::var("QING_LISTEN_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(manifest.app.listen);
        let listen = (manifest.app.listen != 0).then(|| {
            let q = events.clone();
            Arc::new(Listener::new(&manifest.app.name, port, Arc::new(move |v| q.lock().unwrap().push_back(v))))
        });
        let limits = StoreLimitsBuilder::new().memory_size((memory_mb.max(1) as usize) << 20).instances(1).build();
        let mut store = Store::new(engine, Host { full: full.clone(), data, assets, limits, listen: listen.clone(), mdns: manifest.app.mdns.clone(), port, mdns_set: false });
        store.limiter(|h| &mut h.limits);
        store.set_epoch_deadline(30);
        let inst = linker.instantiate(&mut store, &module).with_context(|| format!("{} 起不来", path.display()))?;
        if let Ok(init) = inst.get_typed_func::<(), ()>(&mut store, "qinit") {
            init.call(&mut store, ()).map_err(|e| anyhow!("qinit 出错：{e:#}"))?;
        }
        let tick = inst.get_typed_func::<(), ()>(&mut store, "qing_tick").context("没有导出 qing_tick：不是整屏 app")?;
        let call = (|| {
            Some(Calls {
                memory: inst.get_memory(&mut store, "memory")?,
                alloc: inst.get_typed_func(&mut store, "qing_alloc").ok()?,
                free: inst.get_typed_func(&mut store, "qing_free").ok()?,
                call: inst.get_typed_func(&mut store, "qing_call").ok()?,
            })
        })();
        if let Some(l) = &listen {
            l.start().context("开不了端口（清单的 listen，换一个用 QING_LISTEN_PORT）")?;
            if !manifest.app.mdns.is_empty() && !store.data().mdns_set {
                eprintln!("[mdns] 会回 {}：实例「{}」，端口 {port}（开发机上不发）", manifest.app.mdns, manifest.app.name);
            }
        }
        Ok(Player { store, tick, full, call, events, listen })
    }

    /// 开着的端口（清单写了 `listen`），没有是 None。
    pub fn listening(&self) -> Option<u16> {
        self.listen.as_ref().and_then(|l| l.bound())
    }

    /// 交一个事件：`notifications/qing/event`，和设备上一样走 `qing_call`。
    fn event(&mut self, ev: serde_json::Value) -> Result<()> {
        let Some(c) = &self.call else { return Ok(()) };
        let req = serde_json::to_vec(&serde_json::json!({ "method": "notifications/qing/event", "params": ev }))?;
        let store = &mut self.store;
        let p = c.alloc.call(&mut *store, req.len() as u32)?;
        c.memory.write(&mut *store, p as usize, &req)?;
        let packed = c.call.call(&mut *store, (p, req.len() as u32))?;
        // 旧版 qapp 编的 app 释放回复时会出错（容量和长度不一样），和设备上的运行时一样不管它。
        let _ = c.free.call(&mut *store, ((packed >> 32) as u32, packed as u32));
        Ok(())
    }

    /// 跑一帧：交这一帧的按键，调一次 `qing_tick`。app 出错（panic、死循环）回错，之后不能再跑。
    /// 连接上来的事先交（每个事件一样有一帧的时间）。
    pub fn tick(&mut self, buttons: u32) -> Result<()> {
        loop {
            let Some(ev) = self.events.lock().unwrap().pop_front() else { break };
            self.store.set_epoch_deadline(TICK_DEADLINE_SECS);
            self.event(ev).map_err(|e| anyhow!("app 收事件时出错：{e:#}"))?;
        }
        self.full.set_buttons(buttons);
        self.store.set_epoch_deadline(TICK_DEADLINE_SECS);
        self.tick.call(&mut self.store, ()).map_err(|e| anyhow!("app 在这一帧出错：{e:#}"))
    }

    /// 整屏的状态：取画面、取音调、要声音的样本。
    pub fn full(&self) -> &Arc<Full> {
        &self.full
    }
}

/// 一串名字写成 JSON 数组。名字都是文件名，只要转义引号和反斜杠。
fn serde_json_lite(keys: &[String]) -> String {
    let items: Vec<String> = keys.iter().map(|k| format!("\"{}\"", k.replace('\\', "\\\\").replace('"', "\\\""))).collect();
    format!("[{}]", items.join(","))
}

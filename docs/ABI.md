# app 和运行时的约定（ABI）

app 是一个 `wasm32-unknown-unknown` 模块。`qapp::main!` 把下面这些都接好，用 `qapp` 写的 app 不用碰。
别的语言编的 wasm，照这一页导出、导入就行。

## 模块导出

| 名字 | 签名 | 做什么 |
|---|---|---|
| `memory` | 内存 | 运行时从这里读写 |
| `qinit` | `() -> ()` | 起来时调一次 |
| `qing_alloc` | `(len: u32) -> u32` | 运行时往 app 里放东西前，让 app 分配 |
| `qing_free` | `(ptr: u32, len: u32) -> ()` | 运行时读完 app 回的东西后释放 |
| `qing_call` | `(ptr: u32, len: u32) -> u64` | 一条请求进来，一条回复出去。高 32 位是回复的地址，低 32 位是长度 |
| `qing_tick` | `() -> ()` | 清单写了 `rate_hz` 才有：运行时按这个频率调 |

请求和回复都是 JSON：`{"method", "params"}` 进，`{"result"}` 或 `{"error"}` 出。方法名和 MCP 一样：
`initialize`、`tools/list`、`tools/call`、`qing/check`、`notifications/qing/event`。

## 导入（模块名 `qing`）

运行时给的函数。回一段字节的函数回 `u64`：高 32 位地址、低 32 位长度，0 是没有；这段字节是运行时用 `qing_alloc` 放进去的，归 app。

### 整屏 app 用到的

一个整屏 app（游戏）只用得到这一组。开发机上的装载器（`qfull` 的 `Player`）只接这一组，调了别的就出错。

| 名字 | 签名 | 做什么 |
|---|---|---|
| `frame` | `(pixels: u32, palette: u32) -> i32` | 交一帧：240×160 个字节，一个字节是调色板的一格；调色板 256 格，每格 R G B。0 是收了 |
| `frame565` | `(pixels: u32) -> i32` | 交一帧真彩画面：640×480 个像素，一个像素两个字节，RGB565，高字节在前。清单要写 `frame = "640x480"`。0 是收了，-1 是没写或地址不对 |
| `buttons` | `() -> u32` | 这一帧哪些键按着。位：上 0、下 1、左 2、右 3、A 4、B 5、X 6、Y 7、L1 8、R1 9、Start 10、Select 11 |
| `tone` | `(channel: u32, hz: u32, ms: u32, volume: u32) -> i32` | 声道 0、1 方波，2 三角波，3 噪声。`ms` 是 0 就停；音量 0–100 |
| `tone_wav` | `(ptr: u32, len: u32) -> i32` | 第五个声道放一段 WAV（16 位 PCM，最长 2 秒） |
| `lang` | `() -> u32` | 系统现在用的语言：0 英文，1 中文。人随时能换，出字时再问 |
| `log` | `(ptr: u32, len: u32)` | 记一行日志 |
| `data_read` | `(kptr, klen) -> u64` | 读自己的数据（名字是一个文件名） |
| `data_write` | `(kptr, klen, vptr, vlen) -> i32` | 存。0 是存了 |
| `data_delete` | `(kptr, klen) -> i32` | 删 |
| `data_list` | `() -> u64` | 所有名字，JSON 数组 |
| `asset_read` | `(ptr, len) -> u64` | 读跟着 app 装来的文件（app 目录 `assets/` 下） |
| `now` | `() -> i64` | Unix 秒 |
| `tz_offset` | `() -> i32` | 时区偏移，秒 |

### 在局域网上等别人连进来

清单写了 `listen`，系统开这个端口、收连接，按行交给 app。app 不碰 socket。开发机上的 `Player` 也接这一组。

| 名字 | 签名 | 做什么 |
|---|---|---|
| `conn_send` | `(id: u64, ptr: u32, len: u32) -> i32` | 往一条连接发一行，系统补换行。行里不能有换行，最长 8192 字节。0 是发了 |
| `conn_close` | `(id: u64)` | 关掉一条连接 |
| `mdns_set` | `(ptr: u32, len: u32) -> i32` | `{"instance": "...", "txt": {"k": "v"}}`：系统替它回的 mDNS 的实例名和 TXT。清单要写 `mdns`。0 是换了 |
| `random` | `(len: u32) -> u64` | `len` 个随机字节（最多 4096），从系统的随机源来。不用写清单 |

连接上的事是事件（`notifications/qing/event`）：

- `{"kind": "conn", "id": 1, "what": "open", "peer": "192.168.1.20:51234"}`
- `{"kind": "conn", "id": 1, "what": "line", "text": "..."}`：一行，不带换行。超过 8192 字节的整行丢掉，连接不断
- `{"kind": "conn", "id": 1, "what": "close"}`

规矩：

- 只收局域网（10/8、172.16/12、192.168/16、169.254/16）和本机（127/8）来的连接，别的直接关。
- 同时最多 4 个连接，多的直接关。
- 系统不看内容，不配对。谁能连进来、连进来能做什么，app 自己管（比如用 Noise 握手）。
- 整屏 app 停下（不在前台、合盖）时，系统关掉端口和所有连接，撤掉 mDNS。接着跑时再开。
- 没调 `mdns_set` 时，实例名是 app 的名字，TXT 是空的。
- 开发机上：端口真开，`QING_LISTEN_PORT` 换端口；mDNS 只记日志，不发。连接上的事在每一帧开头交给 app。

### 别的 app 用的

`push`、`page`、`sound`、`sound_stop`、`http`、`http_start`、`setting`、`location`、`name`、`after`、
`record_start`、`record_stop`、`call_start`、`call_send`、`call_stop`、`call_listen`、
`mcu_send`、`mcu_recv`、`lan_send`、`lan_recv`、`watch`。签名见 `qapp/src/lib.rs` 的 `host` 模块。
有的要清单里先要权限（网络、麦克风、通话、MCU、局域网）。

## 清单 `app.toml`

```toml
[app]
name      = "mygame"          # 英文小写
title     = "我的游戏"
version   = "0.1.0"
command   = "bin/mygame.cwasm"  # 给设备编好的本机码；旁边放同名的 .wasm
screen    = "full"            # 整屏页
rate_hz   = 60
memory_mb = 16
cover     = "assets/cover.png"  # 96×80 的 PNG，家上的格子用
listen    = 7342              # 可选：在局域网上等别人连进来的端口，1024 以上，一个 app 一个
mdns      = "_mygame._tcp"    # 可选：系统替它回这个 mDNS 服务，端口就是 listen
```

`.cwasm` 和运行时的 wasmtime 版本绑在一起。编它要用 `qfull` 的 `precompile`，配置一样才读得进来。

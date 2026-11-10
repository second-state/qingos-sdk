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

一个整屏 app（游戏）只用得到这一组。开发机上的装载器（`qingos-host` 的 `Player`）只接这一组，调了别的就出错。

| 名字 | 签名 | 做什么 |
|---|---|---|
| `frame` | `(pixels: u32, palette: u32) -> i32` | 交一帧：240×160 个字节，一个字节是调色板的一格；调色板 256 格，每格 R G B。0 是收了 |
| `buttons` | `() -> u32` | 这一帧哪些键按着。位：上 0、下 1、左 2、右 3、A 4、B 5、X 6、Y 7、L1 8、R1 9、Start 10、Select 11 |
| `tone` | `(channel: u32, hz: u32, ms: u32, volume: u32) -> i32` | 声道 0、1 方波，2 三角波，3 噪声。`ms` 是 0 就停；音量 0–100 |
| `tone_wav` | `(ptr: u32, len: u32) -> i32` | 第五个声道放一段 WAV（16 位 PCM，最长 2 秒） |
| `log` | `(ptr: u32, len: u32)` | 记一行日志 |
| `data_read` | `(kptr, klen) -> u64` | 读自己的数据（名字是一个文件名） |
| `data_write` | `(kptr, klen, vptr, vlen) -> i32` | 存。0 是存了 |
| `data_delete` | `(kptr, klen) -> i32` | 删 |
| `data_list` | `() -> u64` | 所有名字，JSON 数组 |
| `asset_read` | `(ptr, len) -> u64` | 读跟着 app 装来的文件（app 目录 `assets/` 下） |
| `now` | `() -> i64` | Unix 秒 |
| `tz_offset` | `() -> i32` | 时区偏移，秒 |

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
```

`.cwasm` 和运行时的 wasmtime 版本绑在一起。编它要用 `qingos-host` 的 `precompile`，配置一样才读得进来。

# QingOS SDK

QingOS 是一个基于 Linux 内核的 agentic 操作系统。QingOS 上的 app 是一个 wasm 模块加一份清单 `app.toml`，
运行时用 wasmtime 跑它。这个仓库是写 app 要用的几个库：

| 库 | 做什么 |
|---|---|
| `qtypes` | 纯数据：能力声明、卡、页、结果。运行时和 app 共用 |
| `qapp` | 写 app 的库，编成 `wasm32-unknown-unknown`。能力、卡、自己的数据、联网、定时，还有整屏页（`qapp::full`） |
| `qingos-host` | 运行时跑整屏 app 的那一头：画面、按键、合成器、wasmtime 配置，和开发机上只跑整屏 app 的装载器 |

和运行时的约定在 [docs/ABI.md](docs/ABI.md)。

想做游戏，用 [VibeGame SDK](https://github.com/longzhi/vibegame-sdk)：它建在这几个库上面，带命令行、Skill、样板游戏。

## 谁在用

- QingOS 的运行时用 git 依赖钉这个仓库的一个提交。
- [VibeGame SDK](https://github.com/longzhi/vibegame-sdk) 也一样。

改了这里的接口，要去这两处跟上。

开源协议：MIT。

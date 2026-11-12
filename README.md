# QingOS SDK

English · [中文](README.zh-CN.md)

QingOS is an agentic operating system built on the Linux kernel. An app on QingOS is a wasm module plus a manifest, `app.toml`.
The runtime runs it with wasmtime. This repository holds the libraries you need to write apps:

| Library | What it does |
|---|---|
| `qtypes` | Plain data: capability declarations, cards, pages, results. The runtime and apps share it |
| `qapp` | The library for apps, built for `wasm32-unknown-unknown`: capabilities, cards, the app's own data, network, timers, and full-screen pages (`qapp::full`) |
| `qfull` | The runtime side of full-screen apps: screen, buttons, synthesizer, the wasmtime configuration, and a loader on your computer that runs only full-screen apps |

The contract with the runtime is in [docs/ABI.md](docs/ABI.md) (in Chinese).

To make games, use the [VibePlay SDK](https://github.com/second-state/vibeplay-sdk). It is built on these libraries and comes with a command line, a Skill and sample games.

## Who uses it

- The QingOS runtime pins one commit of this repository as a git dependency.
- The [VibePlay SDK](https://github.com/second-state/vibeplay-sdk) does the same.

When you change an interface here, update both of them.

License: MIT.

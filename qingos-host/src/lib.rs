//! QingOS 跑整屏 app 的那一头（ADR 0030、0034），公开在 `qingos-sdk` 里。
//!
//! - [`full`]：整屏页的四样能力。画面、按键、合成器，加上把画面放大到屏上。设备上的运行时用它。
//! - [`config`]：wasmtime 的配置。编本机码和装本机码必须用同一份，不然 `.cwasm` 读不进来。
//! - [`Player`]：只跑整屏 app 的装载器。开发机上的命令行（`vibegame`）用它，设备上不用。
//! - [`precompile`]：把 `.wasm` 编成某台机器的 `.cwasm`（要 `compile` 特性）。

pub mod full;
mod player;

pub use player::Player;

/// 引擎的配置。编本机码和装本机码必须用同一份，不然 `.cwasm` 读不进来。
pub fn config() -> wasmtime::Config {
    let mut cfg = wasmtime::Config::new();
    // 一次调用的超时靠它：编出来的代码在循环和函数入口查 epoch。
    cfg.epoch_interruption(true);
    cfg
}

/// 把 `.wasm` 编成本机码。`target` 是目标三元组（掌机是 `aarch64-unknown-linux-musl`），不写是本机。
#[cfg(feature = "compile")]
pub fn precompile(wasm: &[u8], target: Option<&str>) -> anyhow::Result<Vec<u8>> {
    let mut cfg = config();
    if let Some(t) = target {
        cfg.target(t)?;
    }
    wasmtime::Engine::new(&cfg)?.precompile_module(wasm)
}

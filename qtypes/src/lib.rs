//! 核心和 app 共用的纯数据类型。这个 crate 只依赖 serde，能编到 wasm32。
//!
//! - [`types`]：主体、风险档、能力声明（`ToolSpec`）、意图、设置项、灯环状态、键。
//! - [`card`]：卡的七种词汇。
//! - [`outcome`]：一次调用的结果和它留下的痕迹。

pub mod card;
pub mod outcome;
pub mod page;
pub mod types;

pub use card::{Card, CardBody, ListItem};
pub use outcome::{Effect, Outcome};
pub use page::{Node, Page};
pub use types::{Button, Intent, Place, Principal, RingState, Risk, SettingSpec, ToolCall, ToolSpec};

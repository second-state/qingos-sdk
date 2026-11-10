//! 一次调用的结果：给模型看的、写日志的、给人看的，和它留下的痕迹。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 一次调用在世界上留下的痕迹。守门人把它写进日志。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    /// 只是查了一下，什么都没改。
    None,
    /// 改了，但能改回去。
    Reversible,
    /// 改了，改不回去（钱花了、消息发了）。
    Permanent,
}

impl Effect {
    pub fn as_str(&self) -> &'static str {
        match self {
            Effect::None => "none",
            Effect::Reversible => "reversible",
            Effect::Permanent => "permanent",
        }
    }
    pub fn parse(s: &str) -> Effect {
        match s {
            "reversible" => Effect::Reversible,
            "permanent" => Effect::Permanent,
            _ => Effect::None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Outcome {
    /// 给模型看的结果。
    pub result: Value,
    /// 一句话：日志和「最近做过」里写这个。
    pub summary: String,
    pub effect: Effect,
    /// 给人看的：一张卡，带念出来的话。没有就只写日志不说话（本地动作）。
    pub card: Option<crate::card::Card>,
}

impl Outcome {
    /// 只写日志、不说话的结果（本地动作）。
    pub fn silent(result: Value, summary: impl Into<String>, effect: Effect) -> Self {
        Outcome {
            result,
            summary: summary.into(),
            effect,
            card: None,
        }
    }
    /// 带一张卡的结果。
    pub fn with_card(
        result: Value,
        summary: impl Into<String>,
        effect: Effect,
        card: crate::card::Card,
    ) -> Self {
        Outcome {
            result,
            summary: summary.into(),
            effect,
            card: Some(card),
        }
    }
}

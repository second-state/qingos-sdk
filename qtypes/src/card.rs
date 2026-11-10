//! 卡：App 和系统之间关于"给人看什么"的契约（docs/FACE.md）。
//!
//! 卡是结构化数据，不是像素。每张都能念出来（`spoken`），每张都带谁发的（`from`）。
//! 长什么样是脸的事：灯 + 语音只念 spoken，手环画第一句，桌面终端全画。
//! `ask` 只有守门人能发；别人发的在总线入口就会被丢掉。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CardBody {
    /// 一段话。
    Text {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        body: String,
    },
    /// 几条：几班航班、几件事。
    List {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        items: Vec<ListItem>,
    },
    /// 键值：天气的现在 / 今天 / 明天。
    Kv {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        pairs: Vec<Pair>,
    },
    /// 一张图：门口的画面。data 是 base64，mime 是 image/jpeg 之类。
    Image {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        mime: String,
        data: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caption: Option<String>,
    },
    /// 进度：计时还剩多久、升级到哪了。value 是 0..=1。
    Progress {
        title: String,
        value: f32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    /// 外设报的事。
    Notice { body: String },
    /// 要人拿主意。系统专用。
    Ask {
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scope: Option<String>,
        options: Vec<AskOption>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListItem {
    pub primary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
    /// 带 key 的条目能点。点了 app 收到动作 `item`，带这个 key；念出条目也算。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// 做完了的条目：屏上画成打勾的。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub done: bool,
    /// 这条属于哪一段（「明天 10月13日 星期二」）。段名变了，屏上画一行小字再起一框。没有就是第一段。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
}

impl ListItem {
    pub fn new(primary: impl Into<String>) -> ListItem {
        ListItem {
            primary: primary.into(),
            secondary: None,
            badge: None,
            key: None,
            done: false,
            section: None,
        }
    }
    /// 属于哪一段。
    pub fn section(mut self, s: impl Into<String>) -> ListItem {
        self.section = Some(s.into());
        self
    }
    /// 做完了。
    pub fn done(mut self) -> ListItem {
        self.done = true;
        self
    }
    pub fn secondary(mut self, s: impl Into<String>) -> ListItem {
        self.secondary = Some(s.into());
        self
    }
    pub fn badge(mut self, b: impl Into<String>) -> ListItem {
        self.badge = Some(b.into());
        self
    }
    /// 能点：点了 app 收到 `item` 动作，带这个 key。
    pub fn key(mut self, k: impl Into<String>) -> ListItem {
        self.key = Some(k.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pair {
    pub k: String,
    pub v: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AskOption {
    /// A / B / C，对应三个实体键。
    pub key: String,
    pub label: String,
}

/// 一张卡：谁发的 + 内容 + 念出来的话。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Card {
    /// 能力名或 agent 名。系统画在卡上，App 改不了（守门人会盖掉 App 自己写的）。
    pub from: String,
    #[serde(flatten)]
    pub body: CardBody,
    /// 念出来的话。屏上有的，一定能说。
    pub spoken: String,
    /// app 声明的动作（最多三个）：系统画成统一的按钮，点了 app 收到的是动作名，不是坐标。
    /// 语音也能触发：念出标签就行。`ask` 卡没有这个——它的键是系统的。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<Action>,
    /// app 给这张卡起的记号（提醒的 id）。动作回来时带着它，app 就知道人点的是哪张。
    #[serde(default, rename = "ref", skip_serializing_if = "Option::is_none")]
    pub ref_: Option<String>,
    /// 常驻：不进卡流，钉在家上，同一个 app 的新一张顶掉旧的（带 ref 的按 ref 顶）。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pin: bool,
    /// 到点的事（吃药、开会）：免打扰、人在说话也照样放。静音时还是只画不念。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub urgent: bool,
    /// 念 `spoken` 用的声音文件。app 自己合成（或者它的服务端提前合成），系统只放。没有就只有文字。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<Sound>,
    /// 给版面的设计稿用的几个字段（「time」→「20:00」）：设计稿写 `{next.time}`，系统填进去画。
    /// 只是数据：画在哪、什么字、什么颜色归设计稿，app 不管。
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub fields: std::collections::BTreeMap<String, String>,
}

/// 一段声音：WAV 的字节（base64）。合成不是系统的事，谁的卡谁给声音。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sound {
    /// 现在只认 `audio/wav`。
    pub mime: String,
    pub data: String,
}

/// 卡上的一个动作。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Action {
    /// 回给 app 的名字：snooze。
    pub key: String,
    /// 按钮上的字：再提醒我。
    pub label: String,
}

impl Card {
    /// 谁发的。app 不用写：系统按 app 的名字盖上。系统自己发卡时用它。
    pub fn with_from(mut self, from: impl Into<String>) -> Card {
        self.from = from.into();
        self
    }

    /// 换念出来的话（屏上画的不变）。
    pub fn with_spoken(mut self, s: impl Into<String>) -> Card {
        self.spoken = s.into();
        self
    }

    /// 带上 `spoken` 的声音：app 自己合成好的 WAV 字节。系统放它，不合成。
    pub fn with_sound(mut self, mime: &str, bytes: &[u8]) -> Card {
        self.sound = Some(Sound { mime: mime.to_string(), data: base64_encode(bytes) });
        self
    }

    /// 给文字、清单、键值、图卡加一行标题（通知卡没有标题，不变）。
    pub fn with_title(mut self, t: impl Into<String>) -> Card {
        let t = Some(t.into());
        match &mut self.body {
            CardBody::Text { title, .. } | CardBody::List { title, .. } | CardBody::Kv { title, .. } | CardBody::Image { title, .. } => *title = t,
            _ => {}
        }
        self
    }

    /// 加一个给设计稿用的字段。
    pub fn with_field(mut self, k: impl Into<String>, v: impl Into<String>) -> Card {
        self.fields.insert(k.into(), v.into());
        self
    }

    /// 给这张卡一个记号：动作回来时带着它。
    pub fn with_ref(mut self, r: impl Into<String>) -> Card {
        self.ref_ = Some(r.into());
        self
    }

    /// 常驻卡：钉在家上。
    pub fn pinned(mut self) -> Card {
        self.pin = true;
        self
    }

    /// 到点的事：不攒，现在就放。
    pub fn urgent(mut self) -> Card {
        self.urgent = true;
        self
    }

    /// 一张图：PNG 或 JPEG 的字节。屏上画、手机上画；念的是 spoken。
    pub fn image(
        title: impl Into<String>,
        mime: &str,
        bytes: &[u8],
        caption: Option<String>,
        spoken: impl Into<String>,
    ) -> Card {
        Card {
            from: String::new(),
            body: CardBody::Image {
                title: Some(title.into()),
                mime: mime.to_string(),
                data: base64_encode(bytes),
                caption,
            },
            spoken: spoken.into(),
            actions: vec![],
            ref_: None,
            pin: false,
            urgent: false,
            sound: None,
            fields: Default::default(),
        }
    }

    /// 进度：0 到 1。
    pub fn progress(title: impl Into<String>, value: f32, label: Option<String>, spoken: impl Into<String>) -> Card {
        Card {
            from: String::new(),
            body: CardBody::Progress {
                title: title.into(),
                value: value.clamp(0.0, 1.0),
                label,
            },
            spoken: spoken.into(),
            actions: vec![],
            ref_: None,
            pin: false,
            urgent: false,
            sound: None,
            fields: Default::default(),
        }
    }

    /// 加动作（最多三个，多了的丢掉）。
    pub fn with_actions(mut self, actions: &[(&str, &str)]) -> Card {
        self.actions = actions
            .iter()
            .take(3)
            .map(|(k, l)| Action {
                key: k.to_string(),
                label: l.to_string(),
            })
            .collect();
        self
    }

    pub fn text(
        body: impl Into<String>,
        spoken: impl Into<String>,
    ) -> Card {
        Card {
            from: String::new(),
            body: CardBody::Text {
                title: None,
                body: body.into(),
            },
            spoken: spoken.into(),
            actions: vec![],
            ref_: None,
            pin: false,
            urgent: false,
            sound: None,
            fields: Default::default(),
        }
    }

    pub fn list(
        title: impl Into<String>,
        items: Vec<ListItem>,
        spoken: impl Into<String>,
    ) -> Card {
        Card {
            from: String::new(),
            body: CardBody::List {
                title: Some(title.into()),
                items,
            },
            spoken: spoken.into(),
            actions: vec![],
            ref_: None,
            pin: false,
            urgent: false,
            sound: None,
            fields: Default::default(),
        }
    }

    pub fn kv(
        title: impl Into<String>,
        pairs: Vec<(String, String)>,
        spoken: impl Into<String>,
    ) -> Card {
        Card {
            from: String::new(),
            body: CardBody::Kv {
                title: Some(title.into()),
                pairs: pairs.into_iter().map(|(k, v)| Pair { k, v }).collect(),
            },
            spoken: spoken.into(),
            actions: vec![],
            ref_: None,
            pin: false,
            urgent: false,
            sound: None,
            fields: Default::default(),
        }
    }

    pub fn notice(body: impl Into<String>) -> Card {
        let body = body.into();
        Card {
            from: String::new(),
            spoken: body.clone(),
            body: CardBody::Notice { body },
            actions: vec![],
            ref_: None,
            pin: false,
            urgent: false,
            sound: None,
            fields: Default::default(),
        }
    }

    /// 守门人问人：三个键的含义写在卡上。
    pub fn ask(
        title: impl Into<String>,
        scope: Option<String>,
        options: &[(&str, &str)],
        spoken: impl Into<String>,
    ) -> Card {
        Card {
            from: "qing".into(),
            body: CardBody::Ask {
                title: title.into(),
                scope,
                options: options
                    .iter()
                    .map(|(k, l)| AskOption {
                        key: k.to_string(),
                        label: l.to_string(),
                    })
                    .collect(),
            },
            spoken: spoken.into(),
            actions: vec![],
            ref_: None,
            pin: false,
            urgent: false,
            sound: None,
            fields: Default::default(),
        }
    }

    pub fn is_ask(&self) -> bool {
        matches!(self.body, CardBody::Ask { .. })
    }

    pub fn title(&self) -> Option<&str> {
        match &self.body {
            CardBody::Text { title, .. }
            | CardBody::List { title, .. }
            | CardBody::Kv { title, .. }
            | CardBody::Image { title, .. } => title.as_deref(),
            CardBody::Progress { title, .. } | CardBody::Ask { title, .. } => Some(title),
            CardBody::Notice { .. } => None,
        }
    }

    /// 给只能显示几行字的脸（手表、终端日志）：一行一条。
    pub fn lines(&self) -> Vec<String> {
        match &self.body {
            CardBody::Text { body, .. } => vec![body.clone()],
            CardBody::List { items, .. } => items
                .iter()
                .map(|i| match (&i.secondary, &i.badge) {
                    (Some(s), Some(b)) => format!("{}  {s}  {b}", i.primary),
                    (Some(s), None) => format!("{}  {s}", i.primary),
                    (None, Some(b)) => format!("{}  {b}", i.primary),
                    (None, None) => i.primary.clone(),
                })
                .collect(),
            CardBody::Kv { pairs, .. } => {
                pairs.iter().map(|p| format!("{}：{}", p.k, p.v)).collect()
            }
            CardBody::Image { caption, .. } => {
                vec![caption.clone().unwrap_or_else(|| "（图）".into())]
            }
            CardBody::Progress { value, label, .. } => {
                vec![format!(
                    "{}%{}",
                    (value * 100.0).round() as i32,
                    label
                        .as_deref()
                        .map(|l| format!(" {l}"))
                        .unwrap_or_default()
                )]
            }
            CardBody::Notice { body } => vec![body.clone()],
            CardBody::Ask { scope, options, .. } => {
                let mut v = Vec::new();
                if let Some(s) = scope {
                    v.push(s.clone());
                }
                v.push(
                    options
                        .iter()
                        .map(|o| format!("{} {}", o.key, o.label))
                        .collect::<Vec<_>>()
                        .join(" · "),
                );
                v
            }
        }
    }
}

/// 够用的 base64 解码（标准字母表和 URL 字母表都认，= 和换行跳过）。
pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}



/// 够用的 base64 编码（标准字母表，带 =）。不另拉一个库。
pub fn base64_encode(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_json() {
        let c = Card::kv("北京", vec![("现在".into(), "晴 23°".into())], "北京现在晴，二十三度。").with_from("weather");
        let j = serde_json::to_value(&c).unwrap();
        assert_eq!(j["kind"], "kv");
        assert_eq!(j["from"], "weather");
        let back: Card = serde_json::from_value(j).unwrap();
        assert_eq!(back, c);
        assert_eq!(back.lines(), vec!["现在：晴 23°"]);
        // 没带声音就没有这个字段；带了就原样回来。
        assert!(serde_json::to_value(&c).unwrap().get("sound").is_none());
        let with = c.with_sound("audio/wav", b"RIFF....");
        let back: Card = serde_json::from_value(serde_json::to_value(&with).unwrap()).unwrap();
        assert_eq!(back.sound.as_ref().map(|s| base64_decode(&s.data).unwrap()), Some(b"RIFF....".to_vec()));
    }

    #[test]
    fn ask_is_special() {
        let a = Card::ask(
            "要联网查航班",
            None,
            &[("A", "这一次"), ("B", "以后都行"), ("C", "不行")],
            "要联网查航班，允许吗？",
        );
        assert!(a.is_ask());
        assert_eq!(a.lines(), vec!["A 这一次 · B 以后都行 · C 不行"]);
    }
}

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fmt;

/// 主体：人和 agent 都是主体。授权挂在主体上。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Principal {
    User(String),
    Agent(String),
}

impl Principal {
    pub fn user(name: impl Into<String>) -> Self {
        Principal::User(name.into())
    }
    pub fn agent(name: impl Into<String>) -> Self {
        Principal::Agent(name.into())
    }
    /// 日志和授权里的写法：`user:dragon`、`agent:qing`。
    pub fn id(&self) -> String {
        match self {
            Principal::User(n) => format!("user:{n}"),
            Principal::Agent(n) => format!("agent:{n}"),
        }
    }
    pub fn name(&self) -> &str {
        match self {
            Principal::User(n) | Principal::Agent(n) => n,
        }
    }
}

impl fmt::Display for Principal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.id())
    }
}

/// 动作的三档。守门人只看这个。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    /// 本地可逆：直接做，灯闪一下，不说话。
    Local,
    /// 远端可逆：第一次要授权；做了说一句。
    Remote,
    /// 不可逆：必须按实体键，嘴上说“好”不算。
    Irreversible,
}

impl Risk {
    pub fn label(&self) -> &'static str {
        match self {
            Risk::Local => "本地可逆",
            Risk::Remote => "远端可逆",
            Risk::Irreversible => "不可逆",
        }
    }
}

/// 一个能力对外的样子。字段和 MCP 的 tool 定义对齐，多了擎自己的几样（过 MCP 时放在 `_meta.qing`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema。
    pub parameters: serde_json::Value,
    pub risk: Risk,
    /// 要人确认时念的那句话的模板，`{item}` 会换成参数：“点一份{item}并付款”。
    #[serde(default)]
    pub preview: Option<String>,
    /// 哪些话是找我的（见 intent.rs）。本地可逆的靠它闭环；假模型也靠它。
    #[serde(default)]
    pub intents: Vec<Intent>,
    /// 我有哪些设置项。系统界面替它画，值开机当环境变量给它。
    #[serde(default)]
    pub settings: Vec<SettingSpec>,
    /// 早上打招呼的简报里要不要我（天气、日程）。
    #[serde(default)]
    pub briefing: bool,
}

/// 一条意图：句式加固定参数。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Intent {
    /// `灯 调到 {brightness:percent}`：空格分段，花括号是槽。
    pub pattern: String,
    /// 句式里没有的参数：`关灯` → `{"brightness": 0}`。
    #[serde(default)]
    pub args: serde_json::Value,
}

impl Intent {
    pub fn new(pattern: &str) -> Self {
        Intent {
            pattern: pattern.into(),
            args: serde_json::Value::Null,
        }
    }
    pub fn with(pattern: &str, args: serde_json::Value) -> Self {
        Intent {
            pattern: pattern.into(),
            args,
        }
    }
}

/// 一个设置项。`key` 是给能力进程的环境变量名。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingSpec {
    pub key: String,
    pub label: String,
    /// text | secret | choice
    #[serde(default = "default_setting_kind")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

fn default_setting_kind() -> String {
    "text".into()
}

impl SettingSpec {
    pub fn text(key: &str, label: &str, hint: &str) -> Self {
        SettingSpec {
            key: key.into(),
            label: label.into(),
            kind: "text".into(),
            choices: vec![],
            hint: (!hint.is_empty()).then(|| hint.to_string()),
        }
    }
    pub fn secret(key: &str, label: &str, hint: &str) -> Self {
        SettingSpec {
            kind: "secret".into(),
            ..Self::text(key, label, hint)
        }
    }
    pub fn choice(key: &str, label: &str, choices: &[&str]) -> Self {
        SettingSpec {
            kind: "choice".into(),
            choices: choices.iter().map(|c| c.to_string()).collect(),
            ..Self::text(key, label, "")
        }
    }
}

impl ToolSpec {
    /// 从名字、给守门人念的一句话、参数的 JSON Schema 开始；档位默认远端可逆，其它用链式方法补。
    pub fn new(name: &str, description: &str, parameters: serde_json::Value) -> Self {
        ToolSpec {
            name: name.into(),
            description: description.into(),
            parameters,
            risk: Risk::Remote,
            preview: None,
            intents: vec![],
            settings: vec![],
            briefing: false,
        }
    }
    pub fn risk(mut self, risk: Risk) -> Self {
        self.risk = risk;
        self
    }
    pub fn preview(mut self, template: &str) -> Self {
        self.preview = Some(template.into());
        self
    }
    /// 一条意图句式；参数写死的用 [`ToolSpec::intent_with`]。
    pub fn intent(mut self, pattern: &str) -> Self {
        self.intents.push(Intent::new(pattern));
        self
    }
    pub fn intent_with(mut self, pattern: &str, args: serde_json::Value) -> Self {
        self.intents.push(Intent::with(pattern, args));
        self
    }
    pub fn setting(mut self, s: SettingSpec) -> Self {
        self.settings.push(s);
        self
    }
    pub fn briefing(mut self) -> Self {
        self.briefing = true;
        self
    }

    /// 把模板填上参数。没有模板就用名字加参数。
    pub fn render_preview(&self, args: &serde_json::Value) -> String {
        let Some(t) = &self.preview else {
            return format!("{} {}", self.name, args);
        };
        let mut out = String::new();
        let mut rest = t.as_str();
        while let Some(start) = rest.find('{') {
            out.push_str(&rest[..start]);
            let Some(end) = rest[start..].find('}') else {
                out.push_str(&rest[start..]);
                return out;
            };
            // `{to|你的手机}`：没给 to 就念默认的。
            let (key, default) = match rest[start + 1..start + end].split_once('|') {
                Some((k, d)) => (k, Some(d)),
                None => (&rest[start + 1..start + end], None),
            };
            match &args[key] {
                serde_json::Value::String(s) if !s.trim().is_empty() => out.push_str(s),
                serde_json::Value::String(_) | serde_json::Value::Null => match default {
                    Some(d) => out.push_str(d),
                    None => out.push_str(&format!("{{{key}}}")),
                },
                other => out.push_str(&other.to_string()),
            }
            rest = &rest[start + end + 1..];
        }
        out.push_str(rest);
        out
    }
}

/// 模型要求调用一个能力。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub args: serde_json::Value,
}

/// 灯环六态。设备的脸。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RingState {
    Idle,
    Listening,
    Thinking,
    Working,
    NeedsYou,
    Muted,
}

impl RingState {
    pub fn label(&self) -> &'static str {
        match self {
            RingState::Idle => "息",
            RingState::Listening => "听",
            RingState::Thinking => "想",
            RingState::Working => "做",
            RingState::NeedsYou => "问",
            RingState::Muted => "静",
        }
    }
}

/// 三个实体键。A 确认 / 这次允许，B 以后都允许，C 取消 / 静音。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Button {
    A,
    B,
    C,
}

impl ToolSpec {
    /// MCP 的 tool 定义；我们自己的几样（风险档、预览、意图、设置项、简报）放在 `_meta.qing` 里。
    pub fn to_mcp_json(&self) -> Value {
        json!({
            "name": self.name,
            "description": self.description,
            "inputSchema": self.parameters,
            "annotations": {
                "destructiveHint": self.risk == Risk::Irreversible,
                "openWorldHint": self.risk != Risk::Local
            },
            "_meta": { "qing": {
                "risk": self.risk, "preview": self.preview,
                "intents": self.intents, "settings": self.settings, "briefing": self.briefing,
            } }
        })
    }
}

/// 所在地：设备放在哪。设备设置里存一份，声明了 `location` 的 app 用 `host::location()` 拿。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Place {
    /// 地名：杭州。
    pub name: String,
    /// 省（或州）：浙江。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub region: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub country: String,
    pub lat: f64,
    pub lon: f64,
}

impl Place {
    /// 给人看的一行：杭州 · 浙江 · 中国。省和地名一样（北京）就不重复。
    pub fn label(&self) -> String {
        let mut parts = vec![self.name.as_str()];
        for p in [self.region.as_str(), self.country.as_str()] {
            if !p.is_empty() && !parts.contains(&p) {
                parts.push(p);
            }
        }
        parts.join(" · ")
    }
}

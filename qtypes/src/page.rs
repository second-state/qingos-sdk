//! 页：app 或 agent 交给系统的一整屏。
//! 页是一棵界面树：数据，不是像素。系统照着画；agent 读的也是它。
//! 颜色和字号只能选主题里有的；要任意的样子，以后用画布。

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Page {
    /// 谁的页。运行时盖上 app 的名字，app 改不了。
    #[serde(default)]
    pub from: String,
    /// 页的名字。版面上指向同一个 ref 的按钮会打开这一页；按钮的动作也带着它。
    #[serde(rename = "ref")]
    pub ref_: String,
    pub title: String,
    pub tree: Node,
    /// 念出来的话：没有屏也要能用。
    #[serde(default)]
    pub spoken: String,
}

impl Page {
    pub fn new(ref_: &str, title: &str, tree: Node) -> Self {
        Page { from: String::new(), ref_: ref_.into(), title: title.into(), tree, spoken: String::new() }
    }

    pub fn with_spoken(mut self, s: impl Into<String>) -> Self {
        self.spoken = s.into();
        self
    }

    pub fn with_from(mut self, from: &str) -> Self {
        self.from = from.into();
        self
    }

    /// 页上的字，一行一条。给 agent 读，给没屏的脸用。
    pub fn lines(&self) -> Vec<String> {
        let mut out = vec![self.title.clone()];
        self.tree.lines(&mut out);
        out
    }

    /// 页上的按钮：(名字, key)。
    pub fn buttons(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        self.tree.buttons(&mut out);
        out
    }
}

/// 界面树的一个节点。只有这几种：控件长在系统里，app 只说要哪个。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "el", rename_all = "snake_case")]
pub enum Node {
    /// 竖着排。
    Col {
        items: Vec<Node>,
        #[serde(default)]
        gap: Gap,
    },
    /// 横着排。
    Row {
        items: Vec<Node>,
        #[serde(default)]
        gap: Gap,
    },
    Text {
        text: String,
        #[serde(default)]
        size: Size,
        #[serde(default)]
        tone: Tone,
        #[serde(default, skip_serializing_if = "is_false")]
        bold: bool,
    },
    /// 按钮：点了 app 收到 `action`，key 是这里写的。可以在字前面带一个图标。
    Button {
        label: String,
        key: String,
        #[serde(default, skip_serializing_if = "is_false")]
        primary: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        icon: Option<Icon>,
    },
    /// 一组切换钮：几个里选一个，`on` 是选着的那个的 key。比按钮小，没有箭头（「全部 / 爸爸 / 妈妈」）。
    /// 点了 app 收到 `action`，key 是那一个的 key；换哪个选着由 app 交新的页。
    Choice {
        items: Vec<ChoiceItem>,
        #[serde(default)]
        on: String,
    },
    /// 清单里的一条。开头是「08:30」这样的钟点会单独画一列。带 key 的能点：app 收到 key = item。
    Item {
        primary: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        secondary: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        badge: Option<String>,
        #[serde(default, skip_serializing_if = "is_false")]
        done: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key: Option<String>,
        /// 开头画一个图标。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        icon: Option<Icon>,
    },
    /// 一个图标。大小按字号的名字，颜色按主题的名字。
    Icon {
        icon: Icon,
        #[serde(default)]
        size: Size,
        #[serde(default)]
        tone: Tone,
    },
    /// 小标签：一小段字放在胶囊里，比如「已完成」「下雨 70%」。不能点。
    Tag {
        text: String,
        #[serde(default)]
        tone: Tone,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        icon: Option<Icon>,
    },
    /// 进度条：`value` 从 0 到 1。上面可以有一行字。
    Progress {
        value: f32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    /// 开关：一行字，右边一个开关。点了 app 收到 `action`，key 是这里写的；开还是关由 app 交新的页。
    Switch {
        label: String,
        key: String,
        #[serde(default, skip_serializing_if = "is_false")]
        on: bool,
    },
    /// 一条横线。
    Divider,
    /// 一个框：上面可以有一行小标题，里面的东西之间画线。
    Group {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        items: Vec<Node>,
    },
    /// 占住剩下的地方：后面的东西推到底（竖着）或推到右边（横着）。
    Fill,
}

/// 图标的名字。图标是系统画的，一种线条风格；app 只能选名字。
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Icon {
    // 天气
    Sun,
    Moon,
    Cloud,
    PartlyCloudy,
    Rain,
    Snow,
    Thunder,
    Fog,
    Wind,
    // 常用
    Home,
    Bell,
    Clock,
    Calendar,
    Check,
    Plus,
    Person,
    Phone,
    Message,
    Heart,
    Star,
    Pill,
    List,
    Image,
    Mic,
    Lock,
    Location,
    Warning,
    Info,
}

/// 切换钮里的一个。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ChoiceItem {
    pub label: String,
    pub key: String,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// 字号。从小到大；具体多大由主题和屏定。
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Size {
    Small,
    #[default]
    Body,
    Large,
    Title,
    Huge,
}

/// 颜色。只有主题里的名字。
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    #[default]
    Ink,
    Soft,
    Faint,
    Accent,
    Green,
    Blue,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Gap {
    None,
    Small,
    #[default]
    Normal,
    Large,
}

impl Node {
    pub fn col(items: Vec<Node>) -> Self {
        Node::Col { items, gap: Gap::Normal }
    }

    pub fn row(items: Vec<Node>) -> Self {
        Node::Row { items, gap: Gap::Normal }
    }

    pub fn text(s: impl Into<String>) -> Self {
        Node::Text { text: s.into(), size: Size::Body, tone: Tone::Ink, bold: false }
    }

    pub fn button(label: &str, key: &str) -> Self {
        Node::Button { label: label.into(), key: key.into(), primary: false, icon: None }
    }

    /// 一组切换钮：`(字, key)`，`on` 是选着的 key。
    pub fn choice(items: &[(&str, &str)], on: &str) -> Self {
        Node::Choice { items: items.iter().map(|(l, k)| ChoiceItem { label: l.to_string(), key: k.to_string() }).collect(), on: on.into() }
    }

    pub fn item(primary: impl Into<String>) -> Self {
        Node::Item { primary: primary.into(), secondary: None, badge: None, done: false, key: None, icon: None }
    }

    pub fn icon(icon: Icon) -> Self {
        Node::Icon { icon, size: Size::Body, tone: Tone::Ink }
    }

    pub fn tag(text: impl Into<String>) -> Self {
        Node::Tag { text: text.into(), tone: Tone::Ink, icon: None }
    }

    /// 进度条：`value` 从 0 到 1，超出的按两头算。
    pub fn progress(value: f32) -> Self {
        Node::Progress { value: value.clamp(0.0, 1.0), label: None }
    }

    pub fn switch(label: &str, key: &str, on: bool) -> Self {
        Node::Switch { label: label.into(), key: key.into(), on }
    }

    pub fn divider() -> Self {
        Node::Divider
    }

    pub fn group(title: Option<&str>, items: Vec<Node>) -> Self {
        Node::Group { title: title.map(Into::into), items }
    }

    pub fn fill() -> Self {
        Node::Fill
    }

    /// 行和列：东西之间空多少。
    pub fn gap(mut self, g: Gap) -> Self {
        if let Node::Col { gap, .. } | Node::Row { gap, .. } = &mut self {
            *gap = g;
        }
        self
    }

    pub fn size(mut self, s: Size) -> Self {
        if let Node::Text { size, .. } | Node::Icon { size, .. } = &mut self {
            *size = s;
        }
        self
    }

    pub fn tone(mut self, t: Tone) -> Self {
        if let Node::Text { tone, .. } | Node::Icon { tone, .. } | Node::Tag { tone, .. } = &mut self {
            *tone = t;
        }
        self
    }

    pub fn bold(mut self) -> Self {
        if let Node::Text { bold, .. } = &mut self {
            *bold = true;
        }
        self
    }

    /// 按钮：画成实心的强调色。一排里第一个用。
    pub fn primary(mut self) -> Self {
        if let Node::Button { primary, .. } = &mut self {
            *primary = true;
        }
        self
    }

    pub fn secondary(mut self, s: impl Into<String>) -> Self {
        if let Node::Item { secondary, .. } = &mut self {
            *secondary = Some(s.into());
        }
        self
    }

    pub fn badge(mut self, s: impl Into<String>) -> Self {
        if let Node::Item { badge, .. } = &mut self {
            *badge = Some(s.into());
        }
        self
    }

    pub fn done(mut self) -> Self {
        if let Node::Item { done, .. } = &mut self {
            *done = true;
        }
        self
    }

    /// 按钮、清单的一条、小标签：前面带一个图标。
    pub fn with_icon(mut self, i: Icon) -> Self {
        if let Node::Button { icon, .. } | Node::Item { icon, .. } | Node::Tag { icon, .. } = &mut self {
            *icon = Some(i);
        }
        self
    }

    /// 进度条上面的字。
    pub fn label(mut self, s: impl Into<String>) -> Self {
        if let Node::Progress { label, .. } = &mut self {
            *label = Some(s.into());
        }
        self
    }

    pub fn key(mut self, k: impl Into<String>) -> Self {
        if let Node::Item { key, .. } = &mut self {
            *key = Some(k.into());
        }
        self
    }

    fn lines(&self, out: &mut Vec<String>) {
        match self {
            Node::Col { items, .. } | Node::Row { items, .. } => items.iter().for_each(|n| n.lines(out)),
            Node::Group { title, items } => {
                out.extend(title.clone());
                items.iter().for_each(|n| n.lines(out));
            }
            Node::Text { text, .. } => out.push(text.clone()),
            Node::Item { primary, secondary, badge, .. } => {
                let mut s = primary.clone();
                for x in [secondary, badge].into_iter().flatten() {
                    s.push_str("  ");
                    s.push_str(x);
                }
                out.push(s);
            }
            Node::Button { label, .. } => out.push(format!("[{label}]")),
            Node::Tag { text, .. } => out.push(text.clone()),
            Node::Progress { value, label } => {
                let pct = format!("{}%", (value * 100.0).round() as i64);
                out.push(match label {
                    Some(l) => format!("{l}  {pct}"),
                    None => pct,
                })
            }
            Node::Switch { label, on, .. } => out.push(format!("[{label}：{}]", if *on { "开" } else { "关" })),
            Node::Choice { items, on } => {
                out.push(items.iter().map(|c| if c.key == *on { format!("[{}]", c.label) } else { c.label.clone() }).collect::<Vec<_>>().join(" / "))
            }
            Node::Icon { .. } | Node::Divider | Node::Fill => {}
        }
    }

    fn buttons(&self, out: &mut Vec<(String, String)>) {
        match self {
            Node::Col { items, .. } | Node::Row { items, .. } | Node::Group { items, .. } => items.iter().for_each(|n| n.buttons(out)),
            Node::Button { label, key, .. } | Node::Switch { label, key, .. } => out.push((label.clone(), key.clone())),
            Node::Choice { items, .. } => out.extend(items.iter().map(|c| (c.label.clone(), c.key.clone()))),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_round_trips_and_reads_as_text() {
        let p = Page::new(
            "today",
            "今天的事情",
            Node::col(vec![
                Node::text("10月16日").size(Size::Large).bold(),
                Node::group(None, vec![Node::item("08:00  吃药").secondary("妈妈").badge("待提醒").key("r1")]),
                Node::fill(),
                Node::row(vec![Node::button("我的备忘", "notes").primary(), Node::button("我的闹钟", "alarms")]),
            ]),
        );
        let s = serde_json::to_string(&p).unwrap();
        assert!(s.contains(r#""el":"item""#) && s.contains(r#""ref":"today""#), "{s}");
        let back: Page = serde_json::from_str(&s).unwrap();
        assert_eq!(back, p);
        assert_eq!(p.lines(), vec!["今天的事情", "10月16日", "08:00  吃药  妈妈  待提醒", "[我的备忘]", "[我的闹钟]"]);
        assert_eq!(p.buttons(), vec![("我的备忘".to_string(), "notes".to_string()), ("我的闹钟".to_string(), "alarms".to_string())]);
        // 没写的字段取默认：app 可以只写最少的。
        let n: Node = serde_json::from_str(r#"{"el":"text","text":"hi"}"#).unwrap();
        assert_eq!(n, Node::text("hi"));
    }

    #[test]
    fn icons_and_small_controls_round_trip() {
        let p = Page::new(
            "now",
            "天气",
            Node::col(vec![
                Node::icon(Icon::PartlyCloudy).size(Size::Huge).tone(Tone::Accent),
                Node::tag("下雨 70%").tone(Tone::Blue).with_icon(Icon::Rain),
                Node::divider(),
                Node::progress(1.4).label("电量"),
                Node::switch("早上提醒", "morning", true),
                Node::button("打电话", "call").with_icon(Icon::Phone),
            ]),
        );
        let s = serde_json::to_string(&p).unwrap();
        assert!(s.contains(r#""icon":"partly_cloudy""#), "{s}");
        assert_eq!(serde_json::from_str::<Page>(&s).unwrap(), p);
        assert_eq!(p.lines(), vec!["天气", "下雨 70%", "电量  100%", "[早上提醒：开]", "[打电话]"]);
        assert_eq!(p.buttons().len(), 2);
    }
}

//! 戦略「思想と行動」のツリー。Web 版（apps/client/src/features/strategy/strategy-thought-tree.tsx）と
//! 同じ決まりで、会社の MVV から目標・KR・施策・行動までを 1 本のツリーに並べる。
//!
//! - 段（MVV・方針・目標・KR・施策・行動）は色の札で見分ける。事業を範囲にした MVV は「事業 MVV」。
//! - 事業部は色で見分ける。札は、親と事業部が変わる行にだけ出す。
//! - 状態は言葉から色（注意・進行中・完了・止まり）を決める。目標より上の段は注意が要る状態だけ出す。
//!
//! 画面（`strategy_screen`）はここで作った行をそのまま描く。読み取りだけで、並べ替えや編集はしない。

use std::collections::{HashMap, HashSet};

use serde_json::Value;

/// 組織（事業部・事業・部門）。
#[derive(Clone, Debug, PartialEq)]
pub struct Organization {
    pub id: String,
    pub name: String,
    /// `division`（事業部）・`business`（事業）・`department`（部門）。
    pub kind: String,
    pub parent_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Edge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub relation: String,
}

/// ノード 1 件。画面に要る項目だけを取り出し、元の値も持っておく（詳細で使う）。
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub period_id: String,
    pub department_ids: Vec<String>,
    pub site_ids: Vec<String>,
    pub raw: Value,
}

impl Node {
    pub fn from_value(value: &Value) -> Option<Node> {
        let strings = |v: &Value| -> Vec<String> {
            v.as_array()
                .into_iter()
                .flatten()
                .filter_map(|s| s.as_str().map(str::to_owned))
                .collect()
        };
        Some(Node {
            id: value["id"].as_str()?.to_owned(),
            kind: value["kind"].as_str().unwrap_or_default().to_owned(),
            title: value["title"].as_str().unwrap_or_default().to_owned(),
            period_id: value["periodId"].as_str().unwrap_or_default().to_owned(),
            department_ids: strings(&value["scope"]["departmentIds"]),
            site_ids: strings(&value["scope"]["siteIds"]),
            raw: value.clone(),
        })
    }

    fn text(&self, key: &str) -> Option<String> {
        self.raw[key]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    }

    pub fn owner_name(&self) -> Option<String> {
        self.text("ownerName")
            .or_else(|| {
                self.raw["external"]["owner"]["name"]
                    .as_str()
                    .map(str::to_owned)
            })
            .filter(|s| !s.trim().is_empty())
    }

    /// 本文（MVV・方針の文書など）。無ければ None。
    pub fn content(&self) -> Option<String> {
        strategy_content(&self.raw["content"], 0)
    }
}

/// 読み込んだ「思想と行動」。ページを重ねて読んだ分も、ID でまとめてここに入れる。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Workspace {
    pub version: i64,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub organizations: Vec<Organization>,
    /// 項目ごとの取得状況（`ready` 以外）。項目 ID → 状況の言葉。
    pub states: Vec<(Option<String>, String)>,
    pub periods: Vec<(String, String)>,
    pub staff: Vec<(String, String)>,
    pub complete: bool,
    pub next_cursor: Option<String>,
}

impl Workspace {
    pub fn from_value(value: &Value) -> Workspace {
        let nodes = value["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Node::from_value)
            .collect();
        let edges = value["edges"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|e| {
                Some(Edge {
                    id: e["id"].as_str()?.to_owned(),
                    source: e["source"].as_str()?.to_owned(),
                    target: e["target"].as_str()?.to_owned(),
                    relation: e["relationKind"].as_str().unwrap_or_default().to_owned(),
                })
            })
            .collect();
        let options = &value["scopeOptions"];
        let organizations = if options["organizations"].is_array() {
            &options["organizations"]
        } else {
            &options["departments"]
        };
        let organizations = organizations
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|o| {
                Some(Organization {
                    id: o["id"].as_str()?.to_owned(),
                    name: o["name"].as_str().unwrap_or_default().to_owned(),
                    kind: o["kind"].as_str().unwrap_or_default().to_owned(),
                    parent_id: o["parentId"].as_str().map(str::to_owned),
                })
            })
            .collect();
        let states = value["states"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| {
                let status = s["status"].as_str()?;
                (status != "ready").then(|| {
                    (
                        s["nodeId"].as_str().map(str::to_owned),
                        source_state_label(status).to_owned(),
                    )
                })
            })
            .collect();
        let pairs = |v: &Value| -> Vec<(String, String)> {
            v.as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| {
                    Some((p["id"].as_str()?.to_owned(), p["name"].as_str()?.to_owned()))
                })
                .collect()
        };
        Workspace {
            version: value["version"].as_i64().unwrap_or_default(),
            nodes,
            edges,
            organizations,
            states,
            periods: pairs(&value["periods"]),
            staff: pairs(&options["staff"]),
            complete: value["complete"].as_bool().unwrap_or(true),
            next_cursor: value["nextCursor"].as_str().map(str::to_owned),
        }
    }

    /// 続きのページを足す。版が違えば（途中で変わった）新しいページだけにする（Web 版の mergeWorkspace）。
    pub fn merge(self, page: Workspace) -> Workspace {
        if self.version != page.version {
            return page;
        }
        let mut nodes = self.nodes;
        for node in page.nodes {
            match nodes.iter_mut().find(|n| n.id == node.id) {
                Some(old) => *old = node,
                None => nodes.push(node),
            }
        }
        let mut edges = self.edges;
        for edge in page.edges {
            match edges.iter_mut().find(|e| e.id == edge.id) {
                Some(old) => *old = edge,
                None => edges.push(edge),
            }
        }
        let mut states = self.states;
        states.extend(page.states);
        Workspace {
            nodes,
            edges,
            states,
            organizations: if page.organizations.is_empty() {
                self.organizations
            } else {
                page.organizations
            },
            periods: if page.periods.is_empty() {
                self.periods
            } else {
                page.periods
            },
            staff: if page.staff.is_empty() {
                self.staff
            } else {
                page.staff
            },
            ..page
        }
    }

    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// 上の段（親）の題名。
    pub fn parents_of(&self, id: &str) -> Vec<&Node> {
        self.edges
            .iter()
            .filter(|e| e.target == id && e.relation != "dependsOn")
            .filter_map(|e| self.node(&e.source))
            .collect()
    }

    /// 下の段（子）。
    pub fn children_of(&self, id: &str) -> Vec<&Node> {
        self.edges
            .iter()
            .filter(|e| e.source == id && e.relation != "dependsOn")
            .filter_map(|e| self.node(&e.target))
            .collect()
    }
}

fn source_state_label(status: &str) -> &'static str {
    match status {
        "partial" => "一部のみ取得",
        "stale" => "更新が遅れています",
        "revoked" => "接続が取り消されました",
        "unconnected" => "未接続",
        "forbidden" => "閲覧権限がありません",
        "unavailable" => "現在取得できません",
        _ => "状況を確認してください",
    }
}

/// 状態（`status`）の日本語。
pub fn status_text(status: Option<&str>) -> String {
    let Some(status) = status.filter(|s| !s.is_empty()) else {
        return "未設定".into();
    };
    match status {
        "draft" => "下書き",
        "published" => "公開中",
        "active" => "進行中",
        "completed" => "完了",
        "closed" => "終了",
        "planned" => "予定",
        "canceled" | "cancelled" => "中止",
        "available" => "取得済み",
        "unavailable" => "取得不可",
        "forbidden" | "inaccessible" => "閲覧不可",
        "pending" => "確認待ち",
        "stale" => "更新遅延",
        _ => "状況を確認",
    }
    .into()
}

// ---------------------------------------------------------------- 段

/// ツリーの段の名前。MVV から行動までを同じ言葉で並べる。
pub fn level(kind: &str) -> &'static str {
    match kind {
        "mission" | "vision" | "value" | "company_mvv" => "MVV",
        "division_policy" | "policy" => "方針",
        "objective" => "目標",
        "keyResult" | "key_result" => "KR",
        "initiative" | "project" | "external" => "施策",
        "action" | "issue" => "行動",
        "risk" => "リスク",
        "metric" => "指標",
        _ => "項目",
    }
}

/// 行の段の名前。事業（プロダクト）を範囲にした MVV は「事業 MVV」として、会社の MVV と見分ける。
pub fn level_of(node: &Node, organizations: &[Organization]) -> &'static str {
    let level = level(&node.kind);
    if level != "MVV" {
        return level;
    }
    let business = node.department_ids.iter().any(|id| {
        organizations
            .iter()
            .any(|o| o.kind == "business" && &o.id == id)
    });
    if business { "事業 MVV" } else { level }
}

/// 凡例に並べる段。
pub const LEGEND: &[&str] = &["MVV", "目標", "KR", "施策", "行動"];

/// 段の札の色（地, 文字, 枠）。明るい表示と暗い表示で分ける（Web 版のモックの配色）。
pub fn level_colors(level: &str, dark: bool) -> (u32, u32, Option<u32>) {
    match (level, dark) {
        ("MVV", false) => (0x0F172A, 0xFFFFFF, None),
        ("MVV", true) => (0xF8FAFC, 0x0F172A, None),
        ("事業 MVV", false) => (0xFFFFFF, 0x0F172A, Some(0x0F172A)),
        ("事業 MVV", true) => (0x000000, 0xF8FAFC, Some(0xF8FAFC)),
        ("方針", false) => (0xE2E8F0, 0x334155, None),
        ("方針", true) => (0x1E293B, 0xE2E8F0, None),
        ("目標", false) => (0xEDE9FE, 0x6D28D9, None),
        ("目標", true) => (0x2E1065, 0xC4B5FD, None),
        ("KR", false) => (0xE0F2FE, 0x0369A1, None),
        ("KR", true) => (0x082F49, 0x7DD3FC, None),
        ("施策", false) => (0xFEF3C7, 0x92400E, None),
        ("施策", true) => (0x451A03, 0xFCD34D, None),
        (_, false) => (0xF1F5F9, 0x64748B, None),
        (_, true) => (0x27272A, 0xA1A1AA, None),
    }
}

/// 段ごとの題名の太さ（600 など）と大きさ。上の段ほど太く、行動は細くする。
pub fn title_weight(level: &str) -> (u16, f32) {
    match level {
        "MVV" | "事業 MVV" => (700, 16.0),
        "目標" => (700, 15.0),
        "行動" => (400, 15.0),
        _ => (600, 15.0),
    }
}

// ---------------------------------------------------------------- 事業部

/// 事業部の色。並び順で決め、同じ事業部はどの行でも同じ色にする。
const DIVISION_COLORS: &[u32] = &[0x2563EB, 0xC2410C, 0x0F766E, 0x7C3AED, 0xBE123C, 0x4D7C0F];

#[derive(Clone, Debug, PartialEq)]
pub struct Division {
    pub id: String,
    pub name: String,
    pub color: u32,
}

/// 範囲に使われている事業部と、その色。
pub fn divisions(organizations: &[Organization]) -> Vec<Division> {
    organizations
        .iter()
        .filter(|o| o.kind == "division")
        .enumerate()
        .map(|(i, o)| Division {
            id: o.id.clone(),
            name: o.name.clone(),
            color: DIVISION_COLORS[i % DIVISION_COLORS.len()],
        })
        .collect()
}

/// 組織を親へたどって、属する事業部を返す。事業部が無ければ None。
pub fn division_of(id: &str, organizations: &[Organization]) -> Option<String> {
    let mut seen = HashSet::new();
    let mut current = organizations.iter().find(|o| o.id == id);
    while let Some(org) = current {
        if org.kind == "division" {
            return Some(org.id.clone());
        }
        if !seen.insert(org.id.clone()) {
            return None;
        }
        current = org
            .parent_id
            .as_deref()
            .and_then(|p| organizations.iter().find(|o| o.id == p));
    }
    None
}

// ---------------------------------------------------------------- 状態

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tone {
    Muted,
    Info,
    Ok,
    Warn,
    Danger,
}

/// 状態の言葉から札の色を決める。
pub fn status_tone(status: &str) -> Tone {
    let has = |words: &[&str]| words.iter().any(|w| status.contains(w));
    if has(&["閲覧不可", "取得不可", "中止", "取り消"]) {
        Tone::Danger
    } else if has(&["未確定", "確認", "遅延", "待ち", "未設定"]) {
        Tone::Warn
    } else if has(&["完了", "公開中", "受け入れ"]) {
        Tone::Ok
    } else if has(&["進行中", "作業中"]) {
        Tone::Info
    } else {
        Tone::Muted
    }
}

/// 札の色（地, 文字）。
pub fn tone_colors(tone: Tone, dark: bool) -> (u32, u32) {
    match (tone, dark) {
        (Tone::Muted, false) => (0xF1F5F9, 0x64748B),
        (Tone::Muted, true) => (0x27272A, 0xA1A1AA),
        (Tone::Info, false) => (0xEFF6FF, 0x1D4ED8),
        (Tone::Info, true) => (0x172554, 0x93C5FD),
        (Tone::Ok, false) => (0xF0FDF4, 0x15803D),
        (Tone::Ok, true) => (0x052E16, 0x86EFAC),
        (Tone::Warn, false) => (0xFFFBEB, 0xB45309),
        (Tone::Warn, true) => (0x451A03, 0xFCD34D),
        (Tone::Danger, false) => (0xFEF2F2, 0xB91C1C),
        (Tone::Danger, true) => (0x450A0A, 0xFCA5A5),
    }
}

/// KR などの測定の進み具合（「12 件 / 目標 20 件」）。測定が無ければ None（KR は「測定定義未確定」）。
pub fn node_progress(node: &Node) -> Option<String> {
    let measurement = &node.raw["measurement"];
    if !measurement.is_object() {
        return matches!(node.kind.as_str(), "keyResult" | "key_result")
            .then(|| "測定定義未確定".into());
    }
    let unit = measurement["unit"].as_str().unwrap_or_default();
    let target = format!(
        "{} {unit}",
        measurement["target"].as_str().unwrap_or_default()
    );
    let result = &node.raw["measurementResult"];
    if !result.is_object() {
        return Some(format!("実績未取得 / 目標 {target}"));
    }
    let definition = measurement["binding"]["definitionVersion"].as_str();
    let ok = result["value"].is_string()
        && result["unit"].as_str() == Some(unit)
        && (result["source"].as_str() == Some("manual")
            || result["definitionVersion"].as_str() == definition);
    if !ok {
        return Some(format!("実績を確認 / 目標 {target}"));
    }
    Some(format!(
        "{} {unit} / 目標 {target}",
        result["value"].as_str().unwrap_or_default()
    ))
}

/// 行に出す状態の言葉（Web 版と同じく、進み具合 → 外の状態 → 状態の順）。
pub fn status_of(node: &Node) -> String {
    node_progress(node)
        .or_else(|| node.text("displayStatus"))
        .unwrap_or_else(|| status_text(node.raw["status"].as_str()))
}

// ---------------------------------------------------------------- つながり

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LinkTone {
    Linear,
    Library,
    GoogleDoc,
    Url,
}

impl LinkTone {
    /// 印の文字と色。
    pub fn mark(self) -> (&'static str, u32) {
        match self {
            LinkTone::Linear => ("L", 0x5E6AD2),
            LinkTone::Library => ("Li", 0x0F172A),
            LinkTone::GoogleDoc => ("G", 0x1A73E8),
            LinkTone::Url => ("↗", 0x64748B),
        }
    }
}

fn linear_kind(kind: &str) -> &'static str {
    match kind {
        "initiative" => "Linear イニシアチブ",
        "project" => "Linear プロジェクト",
        "issue" => "Linear Issue",
        _ => "Linear",
    }
}

fn short(text: &str, limit: usize) -> String {
    if text.chars().count() > limit {
        format!("{}…", text.chars().take(limit).collect::<String>())
    } else {
        text.to_owned()
    }
}

/// 行に出す、外のどこにつながっているか。
pub fn link_chips(node: &Node) -> Vec<(String, LinkTone)> {
    let mut links = Vec::new();
    let external = &node.raw["externalRef"];
    if external["provider"].as_str() == Some("linear") {
        links.push((
            linear_kind(external["kind"].as_str().unwrap_or_default()).to_owned(),
            LinkTone::Linear,
        ));
    } else if node.id.starts_with("issue:") {
        links.push((linear_kind("issue").to_owned(), LinkTone::Linear));
    }
    let documents = node.raw["libraryContext"]["documentIds"]
        .as_array()
        .map_or(0, Vec::len);
    if documents > 0 {
        links.push((format!("Library 文書 {documents}"), LinkTone::Library));
    }
    for link in node.raw["documentLinks"].as_array().into_iter().flatten() {
        let kind = link["kind"].as_str().unwrap_or_default();
        let (label, tone) = match kind {
            "google_doc" => ("Google Docs".to_owned(), LinkTone::GoogleDoc),
            "library" => ("Library 文書".to_owned(), LinkTone::Library),
            _ => {
                let title = link["title"].as_str().unwrap_or_default().trim();
                let label = if title.is_empty() {
                    "Web ページ".to_owned()
                } else {
                    short(title, 20)
                };
                (label, LinkTone::Url)
            }
        };
        links.push((label, tone));
    }
    let mut seen = HashSet::new();
    links.retain(|(label, _)| seen.insert(label.clone()));
    links
}

/// 詳細に出す、つないだ文書（題名と URL）。https だけ。
pub fn document_links(node: &Node) -> Vec<(String, String)> {
    node.raw["documentLinks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|link| {
            let url = link["url"].as_str()?;
            if !url.starts_with("https://") {
                return None;
            }
            let kind = match link["kind"].as_str() {
                Some("google_doc") => "Google Docs",
                Some("library") => "Library 文書",
                _ => "Web ページ",
            };
            let title = link["title"]
                .as_str()
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .unwrap_or(kind);
            Some((title.to_owned(), url.to_owned()))
        })
        .collect()
}

// ---------------------------------------------------------------- 本文と題名

/// 本文を取り出す（`body` / `text` / `markdown` などに入れ子で入っていることがある）。
pub fn strategy_content(value: &Value, depth: usize) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_owned());
    }
    if !value.is_object() || depth > 4 {
        return None;
    }
    [
        "body",
        "text",
        "markdown",
        "content",
        "snapshot",
        "publication",
    ]
    .iter()
    .find_map(|key| strategy_content(&value[*key], depth + 1))
    .filter(|t| !t.is_empty())
}

/// 先頭の `---` で囲んだ前置き（frontmatter）を除いた本文。
pub fn strip_frontmatter(body: &str) -> &str {
    let normalized = body.strip_prefix('\u{feff}').unwrap_or(body);
    let Some(rest) = normalized
        .strip_prefix("---\n")
        .or_else(|| normalized.strip_prefix("---\r\n"))
    else {
        return body;
    };
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        offset += line.len();
        if line.trim_end() == "---" {
            return &rest[offset..];
        }
    }
    body
}

fn plain_lines(text: &str) -> Vec<String> {
    strip_frontmatter(text)
        .lines()
        .map(|line| {
            let line = line
                .trim_start_matches('\u{feff}')
                .replace("\\*", "*")
                .replace("\\_", "_");
            let line = line.trim();
            let line = line.trim_start_matches('#').trim_start();
            let line = line.strip_prefix('>').map_or(line, str::trim_start);
            line.replace("**", "").replace("__", "").trim().to_owned()
        })
        .filter(|l| !l.is_empty())
        .collect()
}

fn starts_with_head(line: &str, heads: &[&str]) -> bool {
    let lower = line.to_lowercase();
    heads.iter().any(|h| lower.starts_with(h))
}

const ANY_HEAD: &[&str] = &[
    "mission",
    "vision",
    "value",
    "ミッション",
    "ビジョン",
    "バリュー",
];

/// ミッション・ビジョン・バリューのノードが表す言葉を、元の文書から読む。
/// 会社の MVV の文書は 3 つを 1 つに持つので、題名（文書名）だとどの行も同じになるため。
pub fn mvv_statement(node: &Node) -> Option<String> {
    let heads: &[&str] = match node.kind.as_str() {
        "mission" => &["mission", "ミッション"],
        "vision" => &["vision", "ビジョン"],
        "value" => &["value", "バリュー"],
        _ => return None,
    };
    let text = node.content()?;
    let lines = plain_lines(&text);
    let start = lines.iter().position(|l| starts_with_head(l, heads))?;
    let section: Vec<&String> = lines[start + 1..]
        .iter()
        .take_while(|l| !starts_with_head(l, ANY_HEAD))
        .collect();
    let numbered: Vec<String> = section
        .iter()
        .filter_map(|l| {
            let digits = l.chars().take_while(char::is_ascii_digit).count();
            if digits == 0 {
                return None;
            }
            let rest = &l[digits..];
            let rest = rest
                .strip_prefix('.')
                .or_else(|| rest.strip_prefix('．'))
                .or_else(|| rest.strip_prefix('、'))?;
            let rest = rest.trim();
            (!rest.is_empty() && rest.len() < l.len()).then(|| rest.to_owned())
        })
        .collect();
    let headlines: Vec<String> = if !numbered.is_empty() {
        numbered
    } else {
        section
            .iter()
            .filter(|l| {
                !(l.starts_with('*') || l.starts_with('・') || l.starts_with('-'))
                    && !l.ends_with('）')
                    && l.chars().count() <= 40
            })
            .map(|l| (*l).clone())
            .collect()
    };
    (!headlines.is_empty()).then(|| headlines.join("／"))
}

/// 行に出す名前：自分の言葉があればそれ、無ければ題名。
pub fn display_title(node: &Node) -> String {
    mvv_statement(node)
        .or_else(|| (!node.title.trim().is_empty()).then(|| node.title.clone()))
        .unwrap_or_else(|| "名称未設定".into())
}

// ---------------------------------------------------------------- 並べ方

const KIND_ORDER: &[&str] = &[
    "mission",
    "vision",
    "value",
    "company_mvv",
    "division_policy",
    "policy",
    "objective",
    "keyResult",
    "initiative",
    "project",
    "external",
    "action",
    "issue",
];

fn kind_rank(kind: &str) -> usize {
    KIND_ORDER
        .iter()
        .position(|k| *k == kind)
        .unwrap_or(KIND_ORDER.len())
}

/// 1 つの文書・1 つの範囲から読んだミッション・ビジョン・バリューは 1 つの MVV として、
/// 代表の 1 行（ミッションがあればミッション）にまとめる。ノード ID → 代表の ID。
pub fn shared_mvv_rows(nodes: &[Node]) -> HashMap<String, String> {
    let mut groups: Vec<(String, Vec<&Node>)> = Vec::new();
    for node in nodes {
        if !matches!(node.kind.as_str(), "mission" | "vision" | "value") {
            continue;
        }
        let policy = &node.raw["policy"];
        if !policy.is_object() {
            continue;
        }
        let mut departments = node.department_ids.clone();
        departments.sort();
        let mut sites = node.site_ids.clone();
        sites.sort();
        let key = [
            policy["orgUsername"].as_str().unwrap_or_default(),
            policy["repoUsername"].as_str().unwrap_or_default(),
            policy["recordId"].as_str().unwrap_or_default(),
            &node.period_id,
            &departments.join(","),
            &sites.join(","),
        ]
        .join("|");
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, list)) => list.push(node),
            None => groups.push((key, vec![node])),
        }
    }
    let mut row_of = HashMap::new();
    for (_, group) in groups {
        if group.len() < 2 {
            continue;
        }
        let row = group
            .iter()
            .min_by_key(|n| kind_rank(&n.kind))
            .map(|n| n.id.clone())
            .unwrap_or_default();
        for node in group {
            row_of.insert(node.id.clone(), row.clone());
        }
    }
    row_of
}

/// 親が先、子がその下に来るよう並べ、深さを付ける。親が複数ある子は最初の親の下に 1 回だけ。
pub fn outline<'a>(nodes: &'a [Node], edges: &[Edge]) -> Vec<(&'a Node, usize)> {
    let row_of = shared_mvv_rows(nodes);
    let rep = |id: &str| row_of.get(id).cloned().unwrap_or_else(|| id.to_owned());
    let mut shown: Vec<&Node> = Vec::new();
    for node in nodes {
        if rep(&node.id) == node.id && !shown.iter().any(|n| n.id == node.id) {
            shown.push(node);
        }
    }
    let find = |id: &str| shown.iter().copied().find(|n| n.id == id);
    let mut children: HashMap<String, Vec<&Node>> = HashMap::new();
    let mut has_parent = HashSet::new();
    for edge in edges {
        if edge.relation == "dependsOn" {
            continue;
        }
        let (Some(parent), Some(child)) = (find(&rep(&edge.source)), find(&rep(&edge.target)))
        else {
            continue;
        };
        if parent.id == child.id {
            continue;
        }
        let list = children.entry(parent.id.clone()).or_default();
        if !list.iter().any(|n| n.id == child.id) {
            list.push(child);
        }
        has_parent.insert(child.id.clone());
    }
    let mut sorted = shown.clone();
    sorted.sort_by_key(|n| kind_rank(&n.kind));
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    // 深い木でもスタックがあふれないよう、再帰ではなく自前のスタックでたどる。
    let mut visit = |root: &'a Node, result: &mut Vec<(&'a Node, usize)>| {
        let mut stack = vec![(root, 0usize)];
        while let Some((node, depth)) = stack.pop() {
            if !seen.insert(node.id.clone()) {
                continue;
            }
            result.push((node, depth));
            let mut kids = children.get(&node.id).cloned().unwrap_or_default();
            kids.sort_by_key(|n| kind_rank(&n.kind));
            for kid in kids.into_iter().rev() {
                stack.push((kid, depth + 1));
            }
        }
    };
    for node in &sorted {
        if !has_parent.contains(&node.id) {
            visit(node, &mut result);
        }
    }
    // 輪になっていて根の無いものも落とさない。
    for node in &shown {
        visit(node, &mut result);
    }
    result
}

/// 行動の数（目標・KR・施策の下）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Counts {
    pub total: u64,
    pub held_up: u64,
}

/// 進み具合の応答（`/v1/erp/strategy/progress`）から、目標ごとの行動の数を取る。
pub fn goal_counts(progress: &Value) -> HashMap<String, Counts> {
    progress["goals"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| {
            Some((
                g["goalId"].as_str()?.to_owned(),
                Counts {
                    total: g["counts"]["total"].as_u64().unwrap_or_default(),
                    held_up: g["counts"]["heldUp"].as_u64().unwrap_or_default(),
                },
            ))
        })
        .collect()
}

/// 画面に描く 1 行。描くのに要るものは全部ここで決めておく。
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: String,
    pub depth: usize,
    pub has_children: bool,
    pub open: bool,
    pub level: &'static str,
    pub title: String,
    /// 事業部の札（親と事業部が変わる行だけ）。
    pub division_badge: Option<(String, u32)>,
    /// 目標・事業 MVV の行の左に引く事業部の色の帯。
    pub stripe: Option<u32>,
    /// 事業部より細かい範囲（事業・部門）の名前と、その事業部の色。
    pub scope_names: Vec<(String, Option<u32>)>,
    pub links: Vec<(String, LinkTone)>,
    pub notes: Vec<String>,
    pub owner: Option<String>,
    pub status: Option<(String, Tone)>,
    pub counts: Option<Counts>,
}

/// 開いている行だけを、描く順に並べる（閉じた行の下は除く）。
pub fn rows(
    ws: &Workspace,
    collapsed: &HashSet<String>,
    counts: &HashMap<String, Counts>,
) -> Vec<Row> {
    let orgs = &ws.organizations;
    let divisions = divisions(orgs);
    let outline = outline(&ws.nodes, &ws.edges);
    let mut notes: HashMap<&str, Vec<String>> = HashMap::new();
    for (id, label) in &ws.states {
        if let Some(id) = id {
            let list = notes.entry(id.as_str()).or_default();
            if !list.contains(label) {
                list.push(label.clone());
            }
        }
    }
    let mut division_at: Vec<Option<String>> = Vec::new();
    let mut scope_at: Vec<String> = Vec::new();
    let mut result = Vec::new();
    let mut hidden_below: Option<usize> = None;
    for (index, (node, depth)) in outline.iter().enumerate() {
        let depth = *depth;
        let has_children = outline.get(index + 1).is_some_and(|(_, d)| *d > depth);
        // 事業部と範囲は、閉じて見えない行もたどって覚える（開いたときと同じ札になるように）。
        let own_division = node
            .department_ids
            .iter()
            .find_map(|id| division_of(id, orgs));
        let parent_division = if depth > 0 {
            division_at.get(depth - 1).cloned().flatten()
        } else {
            None
        };
        let division = own_division.clone().or_else(|| parent_division.clone());
        let mut scope_ids = node.department_ids.clone();
        scope_ids.sort();
        let scope_key = scope_ids.join(",");
        let parent_scope = if depth > 0 {
            scope_at.get(depth - 1).cloned().unwrap_or_default()
        } else {
            String::new()
        };
        division_at.truncate(depth);
        division_at.push(division.clone());
        scope_at.truncate(depth);
        scope_at.push(if scope_key.is_empty() {
            parent_scope.clone()
        } else {
            scope_key.clone()
        });

        if let Some(limit) = hidden_below {
            if depth > limit {
                continue;
            }
            hidden_below = None;
        }
        let open = !collapsed.contains(&node.id);
        if has_children && !open {
            hidden_below = Some(depth);
        }

        let info = division
            .as_ref()
            .and_then(|id| divisions.iter().find(|d| &d.id == id));
        let level = level_of(node, orgs);
        let show_division =
            info.is_some() && own_division.is_some() && own_division != parent_division;
        let scope_changed = !scope_key.is_empty() && scope_key != parent_scope;
        let scope_names = if scope_changed {
            node.department_ids
                .iter()
                .filter(|id| !divisions.iter().any(|d| &d.id == *id))
                .filter_map(|id| orgs.iter().find(|o| &o.id == id))
                .map(|o| (o.name.clone(), info.map(|d| d.color)))
                .collect()
        } else {
            Vec::new()
        };
        let is_action = level == "行動";
        let status = status_of(node);
        let tone = status_tone(&status);
        result.push(Row {
            id: node.id.clone(),
            depth,
            has_children,
            open,
            level,
            title: display_title(node),
            division_badge: if show_division {
                info.map(|d| (d.name.clone(), d.color))
            } else {
                None
            },
            stripe: info
                .filter(|_| matches!(level, "目標" | "事業 MVV"))
                .map(|d| d.color),
            scope_names,
            links: link_chips(node),
            notes: notes.get(node.id.as_str()).cloned().unwrap_or_default(),
            owner: node.owner_name(),
            status: (!status.is_empty() && (is_action || tone != Tone::Muted))
                .then_some((status, tone)),
            counts: if is_action {
                None
            } else {
                counts.get(&node.id).copied()
            },
        });
    }
    result
}

/// 「目標まで閉じる」：子を持つ目標を全部閉じる。
pub fn collapse_to_objectives(ws: &Workspace) -> HashSet<String> {
    let outline = outline(&ws.nodes, &ws.edges);
    outline
        .iter()
        .enumerate()
        .filter(|(i, (node, depth))| {
            level(&node.kind) == "目標" && outline.get(i + 1).is_some_and(|(_, d)| d > depth)
        })
        .map(|(_, (node, _))| node.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn workspace() -> Workspace {
        Workspace::from_value(&json!({
            "version": 3,
            "complete": true,
            "scopeOptions": {
                "organizations": [
                    {"id": "div-a", "name": "アニマル", "kind": "division"},
                    {"id": "biz-a", "name": "バナスク", "kind": "business", "parentId": "div-a"},
                    {"id": "div-p", "name": "プラットフォーム", "kind": "division"}
                ],
                "staff": [{"id": "s1", "name": "山田"}]
            },
            "periods": [{"id": "fy", "name": "2026 年度"}],
            "nodes": [
                {"id": "m", "kind": "mission", "title": "会社 MVV", "periodId": "fy",
                 "policy": {"orgUsername": "o", "repoUsername": "r", "recordId": "mvv", "publicationId": "p"},
                 "content": {"body": "---\ntitle: MVV\n---\n## Mission\n世界を良くする\n## Vision\n1. 速く\n2. 強く\n## Value\n誠実"}},
                {"id": "v", "kind": "vision", "title": "会社 MVV", "periodId": "fy",
                 "policy": {"orgUsername": "o", "repoUsername": "r", "recordId": "mvv", "publicationId": "p"}},
                {"id": "o1", "kind": "objective", "title": "犬の居場所を増やす", "periodId": "fy",
                 "scope": {"departmentIds": ["div-a"], "siteIds": []}, "status": "draft"},
                {"id": "k1", "kind": "keyResult", "title": "来店 1000 組", "periodId": "fy",
                 "scope": {"departmentIds": ["biz-a"], "siteIds": []},
                 "measurement": {"baseline": "0", "target": "1000", "unit": "組", "direction": "up",
                   "binding": {"source": "manual", "definitionVersion": "1", "recognitionBasis": "", "taxBasis": ""}},
                 "measurementResult": {"value": "420", "unit": "組", "source": "manual"}},
                {"id": "i1", "kind": "initiative", "title": "ドッグラン拡張", "periodId": "fy",
                 "scope": {"departmentIds": ["biz-a"], "siteIds": []},
                 "externalRef": {"provider": "linear", "workspaceId": "w", "externalId": "x", "kind": "project"},
                 "documentLinks": [{"kind": "google_doc", "title": "計画", "url": "https://docs.google.com/x"}]},
                {"id": "a1", "kind": "action", "title": "柵を発注", "periodId": "fy", "displayStatus": "進行中", "ownerName": "山田"},
                {"id": "o2", "kind": "objective", "title": "Field を売る", "periodId": "fy",
                 "scope": {"departmentIds": ["div-p"], "siteIds": []}}
            ],
            "edges": [
                {"id": "e1", "source": "m", "target": "o1", "relationKind": "supports"},
                {"id": "e1b", "source": "v", "target": "o2", "relationKind": "supports"},
                {"id": "e2", "source": "o1", "target": "k1", "relationKind": "supports"},
                {"id": "e3", "source": "k1", "target": "i1", "relationKind": "supports"},
                {"id": "e4", "source": "i1", "target": "a1", "relationKind": "supports"},
                {"id": "e5", "source": "a1", "target": "o2", "relationKind": "dependsOn"}
            ],
            "states": [{"nodeId": "i1", "source": "linear", "status": "stale"}, {"source": "x", "status": "ready"}]
        }))
    }

    #[test]
    fn mission_vision_value_of_one_document_become_one_row() {
        let ws = workspace();
        let rows = rows(&ws, &HashSet::new(), &HashMap::new());
        let ids: Vec<_> = rows.iter().map(|r| (r.id.as_str(), r.depth)).collect();
        assert_eq!(
            ids,
            vec![
                ("m", 0),
                ("o1", 1),
                ("k1", 2),
                ("i1", 3),
                ("a1", 4),
                ("o2", 1)
            ]
        );
        assert_eq!(rows[0].title, "世界を良くする");
        assert_eq!(rows[0].level, "MVV");
    }

    #[test]
    fn division_badge_shows_only_where_the_division_changes() {
        let ws = workspace();
        let rows = rows(&ws, &HashSet::new(), &HashMap::new());
        let o1 = &rows[1];
        assert_eq!(
            o1.division_badge.as_ref().map(|d| d.0.as_str()),
            Some("アニマル")
        );
        assert_eq!(o1.stripe, Some(0x2563EB));
        // KR は同じ事業部の下の事業なので、事業部の札は出さず事業の名前を出す。
        let k1 = &rows[2];
        assert_eq!(k1.division_badge, None);
        assert_eq!(
            k1.scope_names,
            vec![("バナスク".to_owned(), Some(0x2563EB))]
        );
        // 施策は KR と同じ範囲なので札を省く。
        assert!(rows[3].scope_names.is_empty());
        let o2 = &rows[5];
        assert_eq!(o2.division_badge.as_ref().map(|d| d.1), Some(0xC2410C));
    }

    #[test]
    fn statuses_progress_links_and_notes() {
        let ws = workspace();
        let mut counts = HashMap::new();
        counts.insert(
            "o1".to_owned(),
            Counts {
                total: 3,
                held_up: 1,
            },
        );
        let rows = rows(&ws, &HashSet::new(), &counts);
        // 目標の「下書き」は目立たせない。
        assert_eq!(rows[1].status, None);
        assert_eq!(
            rows[1].counts,
            Some(Counts {
                total: 3,
                held_up: 1
            })
        );
        // 測れている KR の値は、Web 版と同じく行には出さない（詳細で見る）。
        assert_eq!(rows[2].status, None);
        assert_eq!(status_of(ws.node("k1").unwrap()), "420 組 / 目標 1000 組");
        assert_eq!(rows[4].status, Some(("進行中".to_owned(), Tone::Info)));
        assert_eq!(rows[4].owner.as_deref(), Some("山田"));
        assert_eq!(rows[4].counts, None);
        assert_eq!(
            rows[3].links,
            vec![
                ("Linear プロジェクト".to_owned(), LinkTone::Linear),
                ("Google Docs".to_owned(), LinkTone::GoogleDoc)
            ]
        );
        assert_eq!(rows[3].notes, vec!["更新が遅れています".to_owned()]);
    }

    #[test]
    fn collapsing_hides_the_rows_below() {
        let ws = workspace();
        let collapsed = collapse_to_objectives(&ws);
        assert_eq!(collapsed, HashSet::from(["o1".to_owned()]));
        let rows = rows(&ws, &collapsed, &HashMap::new());
        let ids: Vec<_> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["m", "o1", "o2"]);
        assert!(!rows[1].open && rows[1].has_children);
    }

    #[test]
    fn business_mvv_is_told_apart() {
        let ws = workspace();
        let mut node = ws.node("m").unwrap().clone();
        node.department_ids = vec!["biz-a".into()];
        assert_eq!(level_of(&node, &ws.organizations), "事業 MVV");
    }

    #[test]
    fn merge_keeps_nodes_from_both_pages_of_one_version() {
        let first = workspace();
        let mut second = Workspace::from_value(&json!({
            "version": 3,
            "nodes": [{"id": "x", "kind": "action", "title": "追加"}],
            "edges": [],
        }));
        second.complete = true;
        let merged = first.clone().merge(second);
        assert_eq!(merged.nodes.len(), first.nodes.len() + 1);
        assert_eq!(merged.organizations, first.organizations);
        let other = Workspace::from_value(&json!({"version": 4, "nodes": []}));
        assert!(first.merge(other).nodes.is_empty());
    }

    #[test]
    fn tones_follow_the_words() {
        assert_eq!(status_tone("閲覧不可"), Tone::Danger);
        assert_eq!(status_tone("測定定義未確定"), Tone::Warn);
        assert_eq!(status_tone("完了"), Tone::Ok);
        assert_eq!(status_tone("下書き"), Tone::Muted);
    }
}

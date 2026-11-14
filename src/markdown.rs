//! Agent の返事の Markdown を画面の部品にする。
//!
//! 返事は少しずつ届くので、文全体を毎回まとまり（見出し・段落・箇条書き・表…）に
//! 分け直す。まとまりは中身ごとに作り直すので、書き終わったところはそのまま残り、
//! 書いている途中の最後のまとまりだけが変わる。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use whisker::css::{FontStyle, LineHeight, TextDecorationLine, TextDecorationStyle};

use super::*;

/// 文の中の飾りがそろった一続き。
#[derive(Clone, Default, PartialEq, Hash, Debug)]
pub(crate) struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strike: bool,
    pub link: bool,
}

#[derive(Clone, Copy, PartialEq, Hash, Debug)]
pub(crate) enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, PartialEq, Hash, Debug)]
pub(crate) struct Item {
    pub depth: usize,
    pub marker: String,
    pub runs: Vec<Run>,
}

/// 返事のまとまり。
#[derive(Clone, PartialEq, Hash, Debug)]
pub(crate) enum Block {
    Heading(u8, Vec<Run>),
    Paragraph(Vec<Run>),
    List(Vec<Item>),
    Quote(Vec<Run>),
    Code(String),
    Table {
        head: Vec<Vec<Run>>,
        align: Vec<Align>,
        rows: Vec<Vec<Vec<Run>>>,
    },
    Rule,
}

/// 文をまとまりに分ける。
pub(crate) fn parse(src: &str) -> Vec<Block> {
    let lines: Vec<&str> = src.lines().collect();
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();
        if trimmed.is_empty() {
            i += 1;
            continue;
        }
        // コード：``` から ``` まで（閉じる前でも、届いた分を出す）。
        if trimmed.starts_with("```") {
            let mut body = Vec::new();
            i += 1;
            while i < lines.len() && !lines[i].trim().starts_with("```") {
                body.push(lines[i]);
                i += 1;
            }
            i += 1;
            blocks.push(Block::Code(body.join("\n")));
            continue;
        }
        if is_rule(trimmed) {
            blocks.push(Block::Rule);
            i += 1;
            continue;
        }
        if let Some((level, rest)) = heading(trimmed) {
            blocks.push(Block::Heading(level, inline(rest)));
            i += 1;
            continue;
        }
        // 表：| で始まる行のすぐ下に |---| の行がある。
        if trimmed.starts_with('|') && i + 1 < lines.len() {
            if let Some(align) = separator(lines[i + 1].trim()) {
                let head = cells(trimmed);
                let mut rows = Vec::new();
                i += 2;
                while i < lines.len() && lines[i].trim().starts_with('|') {
                    rows.push(cells(lines[i].trim()));
                    i += 1;
                }
                blocks.push(Block::Table { head, align, rows });
                continue;
            }
        }
        if list_item(line).is_some() {
            let mut items = Vec::new();
            while i < lines.len() {
                match list_item(lines[i]) {
                    Some(item) => items.push(item),
                    // 箇条書きの続きの行（字下げされた行）は前の項目につなぐ。
                    None if !lines[i].trim().is_empty()
                        && lines[i].starts_with("  ")
                        && !items.is_empty() =>
                    {
                        let last: &mut Item = items.last_mut().unwrap();
                        last.runs.push(Run { text: "\n".into(), ..Run::default() });
                        last.runs.extend(inline(lines[i].trim()));
                    }
                    None => break,
                }
                i += 1;
            }
            blocks.push(Block::List(items));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('>') {
            let mut text = vec![rest.trim().to_owned()];
            i += 1;
            while i < lines.len() {
                match lines[i].trim().strip_prefix('>') {
                    Some(rest) => text.push(rest.trim().to_owned()),
                    None => break,
                }
                i += 1;
            }
            blocks.push(Block::Quote(inline(&text.join("\n"))));
            continue;
        }
        // 段落：空行かほかのまとまりが始まるまで。行の区切りはそのまま残す。
        let mut text = vec![trimmed.to_owned()];
        i += 1;
        while i < lines.len() {
            let next = lines[i].trim();
            if next.is_empty()
                || next.starts_with("```")
                || is_rule(next)
                || heading(next).is_some()
                || list_item(lines[i]).is_some()
                || next.starts_with('>')
                || (next.starts_with('|')
                    && lines.get(i + 1).and_then(|l| separator(l.trim())).is_some())
            {
                break;
            }
            text.push(next.to_owned());
            i += 1;
        }
        blocks.push(Block::Paragraph(inline(&text.join("\n"))));
    }
    blocks
}

fn is_rule(line: &str) -> bool {
    let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    compact.len() >= 3
        && (compact.chars().all(|c| c == '-')
            || compact.chars().all(|c| c == '*')
            || compact.chars().all(|c| c == '_'))
}

fn heading(line: &str) -> Option<(u8, &str)> {
    let level = line.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&level) && line[level..].starts_with(' ') {
        Some((level as u8, line[level..].trim()))
    } else {
        None
    }
}

fn list_item(line: &str) -> Option<Item> {
    let indent = line.len() - line.trim_start().len();
    let rest = line.trim_start();
    let depth = indent / 2;
    for bullet in ["- ", "* ", "+ ", "・"] {
        if let Some(text) = rest.strip_prefix(bullet) {
            let text = text.trim_start();
            // チェックボックス
            let (marker, text) = if let Some(t) = text.strip_prefix("[ ] ") {
                ("☐", t)
            } else if let Some(t) = text.strip_prefix("[x] ").or_else(|| text.strip_prefix("[X] ")) {
                ("☑", t)
            } else {
                (if depth == 0 { "•" } else { "◦" }, text)
            };
            return Some(Item { depth, marker: marker.into(), runs: inline(text) });
        }
    }
    let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && digits < 4 {
        let after = &rest[digits..];
        if let Some(text) = after.strip_prefix(". ").or_else(|| after.strip_prefix(") ")) {
            return Some(Item {
                depth,
                marker: format!("{}.", &rest[..digits]),
                runs: inline(text.trim_start()),
            });
        }
    }
    None
}

/// `|:---|--:|` の行なら、列ごとの寄せを返す。
fn separator(line: &str) -> Option<Vec<Align>> {
    if !line.contains('-') || !line.contains('|') {
        return None;
    }
    let inner = line.trim().trim_start_matches('|').trim_end_matches('|');
    let mut align = Vec::new();
    for cell in inner.split('|') {
        let cell = cell.trim();
        let dashes = cell.trim_start_matches(':').trim_end_matches(':');
        if dashes.is_empty() || !dashes.chars().all(|c| c == '-') {
            return None;
        }
        align.push(match (cell.starts_with(':'), cell.ends_with(':')) {
            (true, true) => Align::Center,
            (false, true) => Align::Right,
            _ => Align::Left,
        });
    }
    Some(align)
}

fn cells(line: &str) -> Vec<Vec<Run>> {
    let inner = line.trim().trim_start_matches('|');
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    inner.split('|').map(|c| inline(c.trim())).collect()
}

/// 文の中の飾り（**太字**・*斜体*・`コード`・~~取り消し~~・[リンク](URL)）を分ける。
/// 閉じる記号がまだ届いていない記号は、そのまま文字として出す。
pub(crate) fn inline(src: &str) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    let mut cur = Run::default();
    let mut rest = src;
    let flush = |runs: &mut Vec<Run>, cur: &mut Run| {
        if !cur.text.is_empty() {
            let next = Run { text: String::new(), ..cur.clone() };
            match runs.last_mut() {
                Some(last) if Run { text: String::new(), ..last.clone() } == next => {
                    last.text.push_str(&cur.text)
                }
                _ => runs.push(cur.clone()),
            }
            cur.text.clear();
        }
    };
    while let Some(c) = rest.chars().next() {
        // コード：中は飾りなし。
        if c == '`' {
            if let Some(end) = rest[1..].find('`') {
                flush(&mut runs, &mut cur);
                runs.push(Run { text: rest[1..1 + end].to_owned(), code: true, ..Run::default() });
                rest = &rest[end + 2..];
                continue;
            }
        }
        // リンク：[文字](URL) → 文字だけを色付きで。
        if c == '[' {
            if let Some(close) = rest.find("](") {
                if let Some(end) = rest[close + 2..].find(')') {
                    flush(&mut runs, &mut cur);
                    for mut run in inline(&rest[1..close]) {
                        run.link = true;
                        run.bold |= cur.bold;
                        run.italic |= cur.italic;
                        runs.push(run);
                    }
                    rest = &rest[close + 2 + end + 1..];
                    continue;
                }
            }
        }
        let toggles: [(&str, fn(&mut Run) -> &mut bool); 4] = [
            ("**", |r| &mut r.bold),
            ("__", |r| &mut r.bold),
            ("~~", |r| &mut r.strike),
            ("*", |r| &mut r.italic),
        ];
        let mut toggled = false;
        for (mark, field) in toggles {
            if let Some(after) = rest.strip_prefix(mark) {
                let open = !*field(&mut cur.clone());
                // 開く記号は、後ろに閉じる記号があり、すぐ後ろが空白でないときだけ。
                if open && (after.starts_with(' ') || after.is_empty() || !after.contains(mark)) {
                    break;
                }
                flush(&mut runs, &mut cur);
                let f = field(&mut cur);
                *f = !*f;
                rest = after;
                toggled = true;
                break;
            }
        }
        if toggled {
            continue;
        }
        cur.text.push(c);
        rest = &rest[c.len_utf8()..];
    }
    flush(&mut runs, &mut cur);
    runs
}

/// 返事の文を、飾り付きのまとまりとして並べる。
#[component]
pub fn markdown(text: Signal<String>) -> Element {
    render! {
        View(
            style: css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                gap: px(12),
            ),
        ) {
            ForEach(
                each: move || {
                    parse(&text.get())
                        .into_iter()
                        .enumerate()
                        .map(|(i, b)| {
                            let mut h = DefaultHasher::new();
                            b.hash(&mut h);
                            (i, h.finish(), b)
                        })
                        .collect::<Vec<_>>()
                },
                key: |b: &(usize, u64, Block)| (b.0, b.1),
                children: move |b: ReadSignal<(usize, u64, Block)>| block_view(&b.get_untracked().2),
            )
        }
    }
}

const BODY: f32 = 16.0;

fn body_style() -> Css {
    css!(color: hex(LABEL), font_size: px(BODY)).line_height(LineHeight::Number(1.5))
}

/// 飾りの一続きを 1 つの段落（Text）にまとめる。
fn paragraph(runs: &[Run], style: Css) -> Element {
    Text::builder()
        .style(style)
        .body(|b| {
            for run in runs {
                b.push(run_view(run));
            }
        })
        .build()
}

fn run_view(run: &Run) -> Element {
    let mut style = Css::new();
    if run.bold {
        style = style.font_weight(FontWeight::Bold);
    }
    if run.italic {
        style = style.font_style(FontStyle::Italic);
    }
    if run.strike {
        style = style.text_decoration(
            TextDecorationLine::LineThrough,
            TextDecorationStyle::Solid,
            hex(SECONDARY_LABEL),
        );
    }
    if run.link {
        style = style.color(hex(TINT));
    }
    if run.code {
        style = style
            .font_family("Menlo")
            .font_size(px(BODY - 2.0))
            .background_color(hex(FILL));
    }
    Text::builder().value(run.text.clone()).style(style).build()
}

fn block_view(block: &Block) -> Element {
    match block {
        Block::Heading(level, runs) => {
            let size = match level {
                1 => 22.0,
                2 => 20.0,
                3 => 18.0,
                _ => BODY + 1.0,
            };
            paragraph(
                runs,
                css!(color: hex(LABEL), font_size: px(size), font_weight: FontWeight::Bold)
                    .line_height(LineHeight::Number(1.3)),
            )
        }
        Block::Paragraph(runs) => paragraph(runs, body_style()),
        Block::Quote(runs) => View::builder()
            .style(
                css!(padding: (px(2), px(0), px(2), px(12)))
                    .border_left(Border::new().width(px(3)).color(hex(SEPARATOR)).style(BorderStyle::Solid)),
            )
            .body(|b| {
                b.push(paragraph(
                    runs,
                    css!(color: hex(SECONDARY_LABEL), font_size: px(BODY))
                        .line_height(LineHeight::Number(1.5)),
                ))
            })
            .build(),
        Block::Code(code) => View::builder()
            .style(css!(
                padding: (px(10), px(12), px(10), px(12)),
                border_radius: px(10),
                background_color: hex(FILL),
            ))
            .body(|b| {
                b.push(
                    Text::builder()
                        .value(code.clone())
                        .style(
                            css!(color: hex(LABEL), font_size: px(13))
                                .font_family("Menlo")
                                .line_height(LineHeight::Number(1.4)),
                        )
                        .build(),
                )
            })
            .build(),
        Block::Rule => View::builder()
            .style(css!(height: px(0.5), background_color: hex(SEPARATOR)))
            .build(),
        Block::List(items) => View::builder()
            .style(css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                gap: px(4),
            ))
            .body(|b| {
                for item in items {
                    b.push(list_row(item));
                }
            })
            .build(),
        Block::Table { head, align, rows } => table(head, align, rows),
    }
}

fn list_row(item: &Item) -> Element {
    let numbered = item.marker.ends_with('.');
    View::builder()
        .style(css!(
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::FlexStart,
            padding: (px(0), px(0), px(0), px(18.0 * item.depth as f32)),
        ))
        .body(|b| {
            b.push(
                Text::builder()
                    .value(item.marker.clone())
                    .style(
                        css!(
                            color: hex(if numbered { LABEL } else { SECONDARY_LABEL }),
                            font_size: px(BODY),
                            width: px(if numbered { 26 } else { 18 }),
                            flex_shrink: 0.0,
                        )
                        .line_height(LineHeight::Number(1.5)),
                    )
                    .build(),
            );
            b.push(paragraph(
                &item.runs,
                body_style().flex_grow(1.0).flex_shrink(1.0).flex_basis(px(0)),
            ));
        })
        .build()
}

fn table(head: &[Vec<Run>], align: &[Align], rows: &[Vec<Vec<Run>>]) -> Element {
    let columns = head.len().max(1);
    let line = || Border::new().width(px(0.5)).color(hex(SEPARATOR)).style(BorderStyle::Solid);
    let row_view = |cells: &[Vec<Run>], header: bool, first: bool| {
        let mut style = css!(
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Stretch,
        );
        if header {
            style = style.background_color(hex(FILL));
        }
        if !first {
            style = style.border_top(line());
        }
        View::builder()
            .style(style)
            .body(|b| {
                for col in 0..columns {
                    let runs = cells.get(col).cloned().unwrap_or_default();
                    let runs: Vec<Run> = runs
                        .into_iter()
                        .map(|mut r| {
                            r.bold |= header;
                            r
                        })
                        .collect();
                    let text_align = match align.get(col).copied().unwrap_or(Align::Left) {
                        Align::Left => TextAlign::Left,
                        Align::Center => TextAlign::Center,
                        Align::Right => TextAlign::Right,
                    };
                    let mut cell = css!(
                        flex_grow: 1.0,
                        flex_shrink: 1.0,
                        flex_basis: px(0),
                        padding: (px(8), px(10), px(8), px(10)),
                    );
                    if col > 0 {
                        cell = cell.border_left(line());
                    }
                    b.push(
                        View::builder()
                            .style(cell)
                            .body(|b| {
                                b.push(paragraph(
                                    &runs,
                                    css!(color: hex(LABEL), font_size: px(14), text_align: text_align)
                                        .line_height(LineHeight::Number(1.4)),
                                ))
                            })
                            .build(),
                    );
                }
            })
            .build()
    };
    View::builder()
        .style(
            css!(
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                border_radius: px(10),
                overflow: Overflow::Hidden,
            )
            .border(line()),
        )
        .body(|b| {
            b.push(row_view(head, true, true));
            for row in rows {
                b.push(row_view(row, false, false));
            }
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(runs: &[Run]) -> String {
        runs.iter().map(|r| r.text.as_str()).collect()
    }

    #[test]
    fn table_with_alignment() {
        let blocks = parse("今週は203件です。\n\n| 日付 | 件数 |\n|---|---:|\n| 11/9 | 29件 |\n| 11/10 | 29件 |\n\n以上です。");
        assert_eq!(blocks.len(), 3);
        match &blocks[1] {
            Block::Table { head, align, rows } => {
                assert_eq!(plain(&head[1]), "件数");
                assert_eq!(align, &vec![Align::Left, Align::Right]);
                assert_eq!(rows.len(), 2);
                assert_eq!(plain(&rows[1][0]), "11/10");
            }
            other => panic!("not a table: {other:?}"),
        }
    }

    #[test]
    fn inline_marks() {
        let runs = inline("**太字**と*斜体*と`code`と[リンク](https://x)");
        assert!(runs[0].bold && runs[0].text == "太字");
        assert!(runs.iter().any(|r| r.italic && r.text == "斜体"));
        assert!(runs.iter().any(|r| r.code && r.text == "code"));
        assert!(runs.iter().any(|r| r.link && r.text == "リンク"));
    }

    #[test]
    fn unclosed_marks_stay_as_text() {
        assert_eq!(plain(&inline("**途中")), "**途中");
        assert_eq!(plain(&inline("2 * 3 = 6")), "2 * 3 = 6");
    }

    #[test]
    fn lists_and_headings() {
        let blocks = parse("## まとめ\n- 一つ目\n- 二つ目\n  - 下の段\n1. 番号");
        assert!(matches!(&blocks[0], Block::Heading(2, _)));
        match &blocks[1] {
            Block::List(items) => {
                assert_eq!(items.len(), 4);
                assert_eq!(items[2].depth, 1);
                assert_eq!(items[3].marker, "1.");
            }
            other => panic!("not a list: {other:?}"),
        }
    }
}

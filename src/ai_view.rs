use crate::ui;
use anyhow::Result;
use crossterm::{
    event::{KeyCode, KeyModifiers},
    terminal,
};
use std::time::Duration;

pub(crate) struct ChatView {
    identity: String,
    notice: String,
    transcript: Vec<(String, &'static str)>,
    details: String,
    show_details: bool,
    offset: usize,
}

impl ChatView {
    pub fn new(identity: &str) -> Self {
        Self {
            identity: identity.into(),
            notice: String::new(),
            transcript: vec![("说出要整理的资料，或请求保存账号、查看密码。".into(), "2")],
            details: String::new(),
            show_details: false,
            offset: 0,
        }
    }

    pub fn notice(&mut self, notice: &str) {
        self.notice = notice.into();
    }

    pub fn push(&mut self, text: impl Into<String>, color: &'static str) {
        self.transcript.push((text.into(), color));
        self.show_details = false;
        self.offset = 0;
    }

    pub fn details(&mut self, details: String) {
        self.details = details;
        self.show_details = false;
        self.offset = 0;
    }

    fn frame(&mut self, width: usize, height: usize, footer: &[String]) -> Vec<String> {
        if width < 30 || height < 10 {
            return vec![ui::clip("请扩大终端，Esc / Ctrl+C 退出", width)];
        }
        let mut lines = vec![
            ui::color(
                &ui::clip(&format!("ID  /  AI 助手  /  {}", self.identity), width),
                "1;36",
            ),
            ui::color(
                &ui::clip(
                    if self.show_details {
                        "模型详情 · F2 返回对话"
                    } else if self.notice.is_empty() {
                        "对话"
                    } else {
                        &self.notice
                    },
                    width,
                ),
                "2",
            ),
            ui::color(&"─".repeat(width), "2"),
        ];
        let body: Vec<String> = if self.show_details {
            ui::wrap(
                if self.details.is_empty() {
                    "暂无模型详情"
                } else {
                    &self.details
                },
                width,
            )
            .into_iter()
            .map(|line| ui::color(&line, "2"))
            .collect()
        } else {
            self.transcript
                .iter()
                .flat_map(|(text, color)| {
                    ui::wrap(text, width)
                        .into_iter()
                        .map(move |line| ui::color(&line, color))
                })
                .collect()
        };
        let available = height.saturating_sub(lines.len() + footer.len());
        self.offset = self.offset.min(body.len().saturating_sub(available));
        let start = body.len().saturating_sub(available + self.offset);
        lines.extend(body.into_iter().skip(start).take(available));
        lines.resize(height.saturating_sub(footer.len()), String::new());
        lines.extend(footer.iter().map(|line| {
            ui::color(
                &ui::clip(line, width),
                if line.starts_with("› ") {
                    "1;36"
                } else {
                    "2"
                },
            )
        }));
        lines
    }

    fn draw(&mut self, footer: &[String]) -> Result<()> {
        let (w, h) = terminal::size()?;
        // 留出最后一列，避免终端自动换行把固定区域顶走。
        ui::draw(&self.frame((w as usize).saturating_sub(1), h as usize, footer))
    }

    fn navigation(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::PageUp => self.offset = self.offset.saturating_add(5),
            KeyCode::PageDown => self.offset = self.offset.saturating_sub(5),
            KeyCode::F(2) => {
                self.show_details = !self.show_details;
                self.offset = 0;
            }
            _ => return false,
        }
        true
    }

    pub fn input(&mut self) -> Result<Option<String>> {
        let mut submitted = Vec::new();
        let mut input = Vec::new();
        let mut cursor = 0;
        loop {
            let (w, h) = terminal::size()?;
            let width = (w as usize).saturating_sub(1);
            let current = format!(
                "{}▏{}",
                input[..cursor].iter().collect::<String>(),
                input[cursor..].iter().collect::<String>()
            );
            let mut footer = vec!["─".repeat(width)];
            let mut editor = submitted.clone();
            editor.push(current);
            let editor = ui::wrap(&format!("你 › {}", editor.join("\n  │ ")), width);
            footer.extend(
                editor
                    .into_iter()
                    .rev()
                    .take((h as usize / 3).clamp(1, 6))
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev(),
            );
            footer.push("空行提交  PgUp/PgDn 滚动  F2 模型详情  /lock 锁定  /quit 退出".into());
            self.draw(&footer)?;
            if let Some(key) = ui::key(Duration::from_millis(200))? {
                if ui::cancelled(key) {
                    return Ok(None);
                }
                if self.navigation(key.code) {
                    continue;
                }
                match key.code {
                    KeyCode::Enter => {
                        if input.is_empty() {
                            return Ok(Some(submitted.join("\n")));
                        }
                        submitted.push(input.iter().collect::<String>());
                        input.clear();
                        cursor = 0;
                    }
                    KeyCode::Backspace if cursor > 0 => {
                        cursor -= 1;
                        input.remove(cursor);
                    }
                    KeyCode::Backspace if input.is_empty() => {
                        if let Some(previous) = submitted.pop() {
                            input = previous.chars().collect();
                            cursor = input.len();
                        }
                    }
                    KeyCode::Delete if cursor < input.len() => {
                        input.remove(cursor);
                    }
                    KeyCode::Left => cursor = cursor.saturating_sub(1),
                    KeyCode::Right => cursor = (cursor + 1).min(input.len()),
                    KeyCode::Home => cursor = 0,
                    KeyCode::End => cursor = input.len(),
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        input.insert(cursor, c);
                        cursor += 1;
                    }
                    _ => {}
                }
            }
        }
    }

    pub fn status(&mut self, status: &str) -> Result<()> {
        self.draw(&[status.into()])
    }

    pub fn choose(&mut self, title: &str, choices: &[String]) -> Result<Option<usize>> {
        let mut selected = 0;
        loop {
            let (_, h) = terminal::size()?;
            let page = (h as usize / 3).max(1);
            let start = selected / page * page;
            let mut footer = vec![title.into()];
            footer.extend(
                choices
                    .iter()
                    .enumerate()
                    .skip(start)
                    .take(page)
                    .map(|(i, label)| format!("{} {label}", if i == selected { "›" } else { " " })),
            );
            footer.push("↑↓ 选择  Enter 确认  q 退出  PgUp/PgDn 滚动  F2 模型详情".into());
            self.draw(&footer)?;
            if let Some(key) = ui::key(Duration::from_millis(200))? {
                if ui::cancelled(key) || key.code == KeyCode::Char('q') {
                    return Ok(None);
                }
                if self.navigation(key.code) {
                    continue;
                }
                match key.code {
                    KeyCode::Up => selected = (selected + choices.len() - 1) % choices.len(),
                    KeyCode::Down => selected = (selected + 1) % choices.len(),
                    KeyCode::Enter => return Ok(Some(selected)),
                    _ => {}
                }
            }
        }
    }

    pub fn form(&self, title: &str) -> Result<()> {
        ui::draw(&[
            ui::color(
                &format!("ID  /  AI 助手  /  {}", ui::clean(&self.identity)),
                "1;36",
            ),
            ui::clean(title),
            String::new(),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn frames_fit_small_screens_and_details_stay_hidden_until_requested() {
        let colors = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
        let mut view = ChatView::new("alice");
        view.push("AI › 中文内容\n第二行", "2");
        view.details("RAW_JSON_MARKER".into());
        for (width, height) in [(39, 12), (79, 24), (29, 8)] {
            let frame = view.frame(width, height, &["你 ›".into()]);
            assert!(frame.len() <= height);
            assert!(
                frame
                    .iter()
                    .all(|line| colors.replace_all(line, "").width() <= width)
            );
            assert!(!frame.join("\n").contains("RAW_JSON_MARKER"));
        }
        view.navigation(KeyCode::F(2));
        assert!(
            view.frame(79, 24, &[])
                .join("\n")
                .contains("RAW_JSON_MARKER")
        );
    }
}

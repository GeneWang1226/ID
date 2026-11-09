use crate::ui;
use anyhow::{Context, Result, ensure};
use crossterm::{event::KeyCode, terminal};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

struct Entry {
    path: PathBuf,
    directory: bool,
    bytes: u64,
}

fn entries(directory: &Path, hidden: bool) -> Result<Vec<Entry>> {
    let mut result = Vec::new();
    for entry in fs::read_dir(directory).context("无法读取目录")? {
        let entry = entry?;
        if !hidden && entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        if metadata.is_dir() || metadata.is_file() {
            result.push(Entry {
                path,
                directory: metadata.is_dir(),
                bytes: metadata.len(),
            });
        }
    }
    result.sort_by(|a, b| {
        b.directory
            .cmp(&a.directory)
            .then_with(|| a.path.file_name().cmp(&b.path.file_name()))
    });
    Ok(result)
}

fn input_path(text: &str, directory: &Path, home: &Path) -> Result<PathBuf> {
    let text = text.trim();
    ensure!(!text.is_empty(), "请输入文件或目录路径");
    let expand = |value: &str| {
        if value == "~" {
            home.to_path_buf()
        } else if let Some(tail) = value.strip_prefix("~/") {
            home.join(tail)
        } else {
            directory.join(value)
        }
    };
    // 先按原样匹配，避免改写真实文件名中的引号和反斜线；从不执行 shell。
    let literal = expand(text);
    if literal.exists() {
        return Ok(literal);
    }
    let mut value = String::new();
    let mut quote = None;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' | '"' if quote.is_none() => quote = Some(c),
            c if quote == Some(c) => quote = None,
            '\\' if quote != Some('\'') => {
                if quote.is_none() || chars.peek().is_some_and(|c| "\\\"$`".contains(*c)) {
                    value.push(chars.next().context("路径末尾的反斜线需要转义")?);
                } else {
                    value.push(c);
                }
            }
            _ => value.push(c),
        }
    }
    ensure!(quote.is_none(), "路径引号未闭合");
    ensure!(!value.is_empty(), "请输入文件或目录路径");
    Ok(expand(&value))
}

fn completions(text: &str, directory: &Path, home: &Path) -> Result<Vec<Entry>> {
    if text.is_empty() {
        return entries(directory, true);
    }
    let path = input_path(text, directory, home)?;
    let (folder, prefix) = if text.ends_with('/') || text == "~" {
        (path.as_path(), "")
    } else {
        (
            path.parent().context("路径无效")?,
            path.file_name().and_then(|s| s.to_str()).unwrap_or(""),
        )
    };
    Ok(entries(folder, true)?
        .into_iter()
        .filter(|entry| {
            entry
                .path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(prefix)
        })
        .collect())
}

fn size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

pub(crate) fn choose(fallback: &Path) -> Result<Option<PathBuf>> {
    let remote =
        std::env::var_os("SSH_CONNECTION").is_some() || std::env::var_os("SSH_TTY").is_some();
    let title = if remote {
        "添加附件 · 服务器文件"
    } else {
        "添加附件 · 本机文件"
    };
    let Some(mode) = ui::choose(title, &["浏览文件…".into(), "输入 / 粘贴路径…".into()])?
    else {
        return Ok(None);
    };
    let _terminal = ui::Terminal::new(true)?;
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| fallback.to_path_buf());
    let mut directory = home.clone();
    let mut hidden = false;
    let mut rows = entries(&directory, hidden)?;
    let mut selected = 0usize;
    let mut editing = (mode == 1).then(String::new);
    let mut message = String::new();
    loop {
        let (width, height) = terminal::size()?;
        let width = width as usize;
        if width < 30 || height < 12 {
            ui::draw(&[ui::clip("请扩大终端，Esc 取消", width)])?;
            if ui::key(Duration::from_millis(200))?.is_some_and(ui::cancelled) {
                return Ok(None);
            }
            continue;
        }
        let page_size = (height as usize).saturating_sub(10).max(1);
        let page = selected / page_size;
        let mut lines = vec![
            ui::color(&ui::clip(title, width), "1;36"),
            ui::color(
                &ui::clip(&format!("目录：{}", directory.display()), width),
                "36",
            ),
            ui::color(
                &ui::clip(
                    if remote {
                        "SSH：电脑上的文件需先经 SFTP / trzsz 上传到服务器。"
                    } else {
                        "选择文件后加密存入 KDBX，保留原文件。"
                    },
                    width,
                ),
                "2",
            ),
            String::new(),
        ];
        if rows.is_empty() {
            lines.push(ui::color("（空目录；h 显示隐藏文件）", "2"));
        }
        for (i, row) in rows
            .iter()
            .enumerate()
            .skip(page * page_size)
            .take(page_size)
        {
            let label = format!(
                "{} {} {}{}",
                if i == selected { "›" } else { " " },
                if row.directory { "📁" } else { "📄" },
                row.path.file_name().unwrap().to_string_lossy(),
                if row.directory {
                    "/".into()
                } else {
                    format!("  {}", size(row.bytes))
                }
            );
            lines.push(ui::color(
                &ui::clip(&label, width),
                if i == selected {
                    "1;36"
                } else if row.directory {
                    "36"
                } else {
                    "0"
                },
            ));
        }
        let detail = if let Some(text) = &editing {
            format!("路径 › {text}▏")
        } else if let Some(row) = rows.get(selected) {
            format!("选中：{}", row.path.display())
        } else {
            String::new()
        };
        let detail = ui::wrap(&detail, width);
        lines.extend_from_slice(&detail[detail.len().saturating_sub(2)..]);
        lines.push(ui::color(&ui::clip(&message, width), "33"));
        lines.push(ui::color(
            &ui::clip(
                if editing.is_some() {
                    "Tab 补全  Enter 打开目录 / 选择文件"
                } else {
                    "↑↓ 选择  Enter 打开  ← 上级  q 返回"
                },
                width,
            ),
            "2",
        ));
        lines.push(ui::color(
            &ui::clip(
                &if editing.is_some() {
                    "Esc 返回浏览  Ctrl+C 取消".into()
                } else {
                    format!(
                        "PgUp/PgDn 翻页  p 路径  h 隐藏  · 第 {}/{} 页",
                        page + 1,
                        rows.len().div_ceil(page_size).max(1)
                    )
                },
                width,
            ),
            "2",
        ));
        ui::draw(&lines)?;
        let Some(key) = ui::key(Duration::from_millis(200))? else {
            continue;
        };
        if ui::cancelled(key) {
            if key.code == KeyCode::Esc && editing.take().is_some() {
                message.clear();
                continue;
            }
            return Ok(None);
        }
        let target = if let Some(text) = &mut editing {
            match key.code {
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    text.push(c);
                    message.clear();
                    None
                }
                KeyCode::Backspace => {
                    text.pop();
                    None
                }
                KeyCode::Tab => {
                    match completions(text, &directory, &home) {
                        Ok(candidates) if !candidates.is_empty() => {
                            let labels = candidates
                                .iter()
                                .map(|e| {
                                    format!(
                                        "{}{}",
                                        e.path.display(),
                                        if e.directory { "/" } else { "" }
                                    )
                                })
                                .collect::<Vec<_>>();
                            let index = if candidates.len() == 1 {
                                Some(0)
                            } else {
                                ui::choose("路径补全", &labels)?
                            };
                            if let Some(i) = index {
                                *text = labels[i].clone();
                            }
                        }
                        Ok(_) => message = "没有匹配的路径".into(),
                        Err(error) => message = error.to_string(),
                    }
                    None
                }
                KeyCode::Enter => match input_path(text, &directory, &home) {
                    Ok(path) => Some(path),
                    Err(error) => {
                        message = error.to_string();
                        None
                    }
                },
                _ => None,
            }
        } else {
            match key.code {
                KeyCode::Char('q') => return Ok(None),
                KeyCode::Up => {
                    selected = selected.saturating_sub(1);
                    None
                }
                KeyCode::Down => {
                    selected = (selected + 1).min(rows.len().saturating_sub(1));
                    None
                }
                KeyCode::PageUp => {
                    selected = selected.saturating_sub(page_size);
                    None
                }
                KeyCode::PageDown => {
                    selected = (selected + page_size).min(rows.len().saturating_sub(1));
                    None
                }
                KeyCode::Home => {
                    selected = 0;
                    None
                }
                KeyCode::End => {
                    selected = rows.len().saturating_sub(1);
                    None
                }
                KeyCode::Char('p' | '/') => {
                    editing = Some(String::new());
                    None
                }
                KeyCode::Char('h') => {
                    hidden = !hidden;
                    Some(directory.clone())
                }
                KeyCode::Left | KeyCode::Backspace => directory.parent().map(Path::to_path_buf),
                KeyCode::Enter => rows.get(selected).map(|e| e.path.clone()),
                _ => None,
            }
        };
        if let Some(path) = target {
            if path.is_dir() {
                match entries(&path, hidden) {
                    Ok(next) => {
                        directory = path.canonicalize()?;
                        rows = next;
                        selected = 0;
                        editing = None;
                        message.clear();
                    }
                    Err(error) => message = error.to_string(),
                }
            } else if path.is_file() {
                path.to_str().context("文件路径必须是有效的 UTF-8")?;
                return Ok(Some(path));
            } else {
                message = if remote {
                    "服务器上找不到该文件；电脑文件需先上传。"
                } else {
                    "文件不存在或不是普通文件"
                }
                .into();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_and_completion_are_literal_and_handle_spaces_quotes_and_unicode() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let file = root.join("恢复 code's.pdf");
        fs::write(&file, "synthetic").unwrap();
        assert_eq!(input_path("~/恢复 code's.pdf", root, root).unwrap(), file);
        assert_eq!(input_path("\"恢复 code's.pdf\"", root, root).unwrap(), file);
        assert_eq!(
            input_path("'恢复 code'\"'\"'s.pdf'", root, root).unwrap(),
            file
        );
        assert_eq!(input_path("恢复\\ code\\'s.pdf", root, root).unwrap(), file);
        assert_eq!(completions("恢复", root, root).unwrap()[0].path, file);
        assert!(input_path("'unfinished", root, root).is_err());
        assert!(input_path("  ", root, root).is_err());
        assert_eq!(
            input_path("$(touch sentinel)", root, root).unwrap(),
            root.join("$(touch sentinel)")
        );
        assert!(!root.join("sentinel").exists());
    }

    #[test]
    fn listing_sorts_directories_first_and_filters_hidden_and_special_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::write(root.join("a.pdf"), "synthetic").unwrap();
        fs::write(root.join(".hidden"), "synthetic").unwrap();
        fs::create_dir(root.join("z folder")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/dev/null", root.join("device")).unwrap();
        let rows = entries(root, false).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows[0].directory);
        assert_eq!(rows[1].bytes, 9);
        assert_eq!(entries(root, true).unwrap().len(), 3);
        assert!(entries(&root.join("missing"), false).is_err());
        assert!(completions("missing/", root, root).is_err());
    }
}

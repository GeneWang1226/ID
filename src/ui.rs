use crate::vault::Secret;
use anyhow::{Result, bail, ensure};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::{
    cell::Cell,
    io::{self, IsTerminal, Write},
    time::Duration,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use zeroize::Zeroizing;

pub fn require_terminal() -> Result<()> {
    ensure!(
        io::stdin().is_terminal() && io::stdout().is_terminal(),
        "此操作需要交互终端；拒绝向文件或管道输出密码"
    );
    Ok(())
}
pub fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}
pub fn color(text: &str, code: &str) -> String {
    if io::stdout().is_terminal()
        && std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM").unwrap_or_default() != "dumb"
    {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_owned()
    }
}
pub fn clip(text: &str, columns: usize) -> String {
    let text = clean(text);
    if text.width() <= columns {
        return text;
    }
    let mut result = String::new();
    let mut used = 0;
    for c in text.chars() {
        let n = c.width().unwrap_or(0);
        if used + n > columns.saturating_sub(1) {
            break;
        }
        result.push(c);
        used += n;
    }
    if columns > 0 {
        result.push('…');
    }
    result
}
pub fn wrap(text: &str, columns: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        let mut used = 0;
        for c in clean(paragraph).chars() {
            let n = c.width().unwrap_or(0);
            if !line.is_empty() && used + n > columns.max(1) {
                lines.push(std::mem::take(&mut line));
                used = 0;
            }
            line.push(c);
            used += n;
        }
        lines.push(line);
    }
    lines
}

pub struct Terminal {
    alternate: bool,
    raw: bool,
}
thread_local! {
    static ALTERNATE_SCREEN: Cell<bool> = const { Cell::new(false) };
}
pub fn is_fullscreen() -> bool {
    ALTERNATE_SCREEN.get()
}
impl Terminal {
    pub fn new(alternate: bool) -> Result<Self> {
        require_terminal()?;
        let raw = !terminal::is_raw_mode_enabled()?;
        if raw {
            terminal::enable_raw_mode()?;
        }
        let alternate = alternate && !ALTERNATE_SCREEN.get();
        let guard = Self { alternate, raw };
        if alternate {
            execute!(io::stdout(), EnterAlternateScreen, cursor::Hide)?;
            ALTERNATE_SCREEN.set(true);
        }
        Ok(guard)
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        if self.alternate {
            let _ = execute!(io::stdout(), cursor::Show, LeaveAlternateScreen);
            ALTERNATE_SCREEN.set(false);
        }
        if self.raw {
            let _ = terminal::disable_raw_mode();
        }
    }
}
pub fn key(timeout: Duration) -> Result<Option<event::KeyEvent>> {
    if event::poll(timeout)?
        && let Event::Key(k) = event::read()?
        && k.kind != KeyEventKind::Release
    {
        return Ok(Some(k));
    }
    Ok(None)
}
pub fn cancelled(key: event::KeyEvent) -> bool {
    key.code == KeyCode::Esc
        || key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}
pub fn draw(lines: &[String]) -> Result<()> {
    execute!(io::stdout(), cursor::MoveTo(0, 0), Clear(ClearType::All))?;
    print!("{}", lines.join("\r\n"));
    io::stdout().flush()?;
    Ok(())
}
pub fn choose(title: &str, choices: &[String]) -> Result<Option<usize>> {
    choose_from(title, choices, 0)
}
pub fn choose_from(title: &str, choices: &[String], selected: usize) -> Result<Option<usize>> {
    if choices.is_empty() {
        return Ok(None);
    }
    let terminal = Terminal::new(true)?;
    if !terminal.alternate {
        execute!(io::stdout(), cursor::Hide)?;
    }
    let mut selected = selected.min(choices.len() - 1);
    let choice = loop {
        let (w, h) = terminal::size()?;
        let page = (h as usize).saturating_sub(5).max(1);
        let start = selected / page * page;
        let mut lines = vec![color(&clip(title, w as usize), "1;36"), String::new()];
        for (i, label) in choices.iter().enumerate().skip(start).take(page) {
            let text = format!(
                "{} {}",
                if i == selected { "›" } else { " " },
                clip(label, (w as usize).saturating_sub(2))
            );
            lines.push(if i == selected {
                color(&text, "1;36")
            } else {
                text
            });
        }
        lines.push(color(&clip("↑↓ 选择  Enter 确认  q 返回", w as usize), "2"));
        draw(&lines)?;
        if let Some(k) = key(Duration::from_millis(200))? {
            match k.code {
                _ if cancelled(k) => break None,
                KeyCode::Char('q') => break None,
                KeyCode::Up | KeyCode::Char('k') => {
                    selected = (selected + choices.len() - 1) % choices.len()
                }
                KeyCode::Down | KeyCode::Char('j') => selected = (selected + 1) % choices.len(),
                KeyCode::Enter => break Some(selected),
                _ => {}
            }
        }
    };
    if !terminal.alternate {
        execute!(io::stdout(), cursor::MoveTo(0, 0), Clear(ClearType::All))?;
    }
    Ok(choice)
}
pub fn confirm(title: &str) -> Result<bool> {
    Ok(choose(title, &["取消".into(), "确认".into()])? == Some(1))
}

pub fn input(prompt: &str) -> Result<String> {
    require_terminal()?;
    let _terminal = Terminal::new(false)?;
    print!("{}", clean(prompt));
    io::stdout().flush()?;
    let mut result = String::new();
    loop {
        if let Some(k) = key(Duration::from_millis(200))? {
            match k.code {
                _ if cancelled(k) => {
                    print!("\r\n");
                    bail!("已取消");
                }
                KeyCode::Enter => {
                    print!("\r\n");
                    return Ok(result);
                }
                KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => {
                    result.push(c);
                    print!("{c}");
                    io::stdout().flush()?;
                }
                KeyCode::Backspace => {
                    if let Some(c) = result.pop() {
                        for _ in 0..c.width().unwrap_or(0) {
                            print!("\x08 \x08");
                        }
                        io::stdout().flush()?;
                    }
                }
                _ => {}
            }
        }
    }
}

pub fn secret(prompt: &str) -> Result<Secret> {
    require_terminal()?;
    let _terminal = Terminal::new(false)?;
    let fullscreen = is_fullscreen();
    if fullscreen {
        execute!(io::stdout(), cursor::Hide)?;
    } else {
        print!("{}", clean(prompt));
        io::stdout().flush()?;
    }
    let mut result = Secret::empty();
    let mut size = None;
    loop {
        if fullscreen {
            let current = terminal::size()?;
            if size != Some(current) {
                let (lines, _) = secret_frame(prompt, current.0, current.1);
                draw(&lines)?;
                size = Some(current);
                secret_mask(!result.is_empty(), current)?;
            }
        }
        let was_empty = result.is_empty();
        if let Some(k) = key(Duration::from_millis(200))? {
            match k.code {
                _ if cancelled(k) => {
                    if fullscreen {
                        draw(&[])?;
                    } else {
                        print!("\r\n");
                    }
                    bail!("已取消");
                }
                KeyCode::Enter => {
                    if fullscreen {
                        draw(&[])?;
                    } else {
                        print!("\r\n");
                    }
                    return Ok(result);
                }
                KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                    result = Secret::empty();
                }
                KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => {
                    let mut encoded = Zeroizing::new([0; 4]);
                    result.push(c.encode_utf8(&mut *encoded).as_bytes())?;
                }
                KeyCode::Backspace => result.pop_char(),
                _ => {}
            }
        }
        if fullscreen && was_empty != result.is_empty() {
            secret_mask(!result.is_empty(), size.unwrap())?;
        }
    }
}

fn secret_frame(prompt: &str, width: u16, height: u16) -> (Vec<String>, Option<(u16, u16, usize)>) {
    if width < 24 || height < 9 {
        return (vec![clip("请扩大终端，Esc 取消", width as usize)], None);
    }
    let columns = (width as usize - 4).min(60);
    let inner = columns - 4;
    let left = (width as usize - columns) / 2;
    let top = (height as usize - 7) / 2;
    let row = |text: &str| {
        let text = clip(text, inner);
        format!(
            "{}│ {text}{} │",
            " ".repeat(left),
            " ".repeat(inner - text.width())
        )
    };
    let border = |start: char, end: char| {
        color(
            &format!(
                "{}{start}{}{end}",
                " ".repeat(left),
                "─".repeat(columns - 2)
            ),
            "36",
        )
    };
    let mut lines = vec![color("ID  /  安全输入", "1;36")];
    lines.resize(top, String::new());
    lines.extend([
        border('╭', '╮'),
        color(&row(prompt.trim()), "1"),
        row(""),
        row("› "),
        row(""),
        color(&row("Enter 确认  Esc 取消  Ctrl+U 清空"), "2"),
        border('╰', '╯'),
    ]);
    (lines, Some(((left + 2) as u16, (top + 3) as u16, inner)))
}

fn secret_mask(entered: bool, size: (u16, u16)) -> Result<()> {
    if let (_, Some((column, row, width))) = secret_frame("", size.0, size.1) {
        // 固定圆点不反映密码长度，真实输入始终只留在 Secret 缓冲区。
        let text = if entered {
            "› ••••••••"
        } else {
            "› "
        };
        execute!(io::stdout(), cursor::MoveTo(column, row))?;
        print!(
            "{}",
            color(&format!("{text}{}", " ".repeat(width - text.width())), "36")
        );
        io::stdout().flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cjk_wrap_and_clip_never_inject_controls() {
        assert_eq!(clip("中文abc", 5), "中文…");
        assert_eq!(wrap("中文\nabc", 3), vec!["中", "文", "abc"]);
        assert!(!clip("x\x1b[2J", 20).contains('\x1b'));
        assert!(
            wrap("备注".repeat(30).as_str(), 17)
                .iter()
                .all(|line| line.width() <= 17)
        );
    }

    #[test]
    fn secret_dialog_fits_resized_terminals_without_secret_values() {
        for (width, height) in [(8, 4), (24, 9), (40, 12), (100, 30)] {
            let (lines, position) = secret_frame("主密码:\x1b[2J", width, height);
            assert!(lines.len() <= height as usize);
            // NO_COLOR is not needed in unit tests: stdout is a pipe.
            assert!(lines.iter().all(|line| line.width() <= width as usize));
            assert!(!lines.join("").contains('\x1b'));
            if let Some((column, row, inner)) = position {
                assert!((row as usize) < lines.len());
                assert!(column as usize + inner < width as usize);
                assert!(inner >= "› ••••••••".width());
            }
        }
    }
}

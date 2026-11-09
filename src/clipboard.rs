use crate::{Project, config, identity, store, ui, vault::Session};
use anyhow::{Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use crossterm::{
    cursor,
    event::KeyCode,
    execute,
    terminal::{self, Clear, ClearType},
};
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Write},
    time::{Duration, Instant},
};
use unicode_width::UnicodeWidthChar;
use zeroize::Zeroizing;

fn osc(payload: &str) -> Result<()> {
    let mut out = io::stdout().lock();
    if std::env::var_os("TMUX").is_some() {
        out.write_all(b"\x1bPtmux;\x1b")?;
    }
    out.write_all(b"\x1b]52;c;")?;
    out.write_all(payload.as_bytes())?;
    out.write_all(b"\x07")?;
    if std::env::var_os("TMUX").is_some() {
        out.write_all(b"\x1b\\")?;
    }
    out.flush()?;
    Ok(())
}
struct ClearClipboard;
impl Drop for ClearClipboard {
    fn drop(&mut self) {
        let _ = osc("");
    }
}
fn remote() -> bool {
    std::env::var_os("SSH_CONNECTION").is_some() || std::env::var_os("SSH_TTY").is_some()
}

pub fn check(project: &Project, force: bool) -> Result<String> {
    ui::require_terminal()?;
    if !remote() {
        return Ok("native".into());
    }
    let mut fingerprint = std::env::var("SSH_CONNECTION").unwrap_or_default();
    fingerprint.push_str(&std::env::var("SSH_TTY").unwrap_or_default());
    fingerprint.push_str(&std::env::var("TERM").unwrap_or_default());
    // Linux 下用终端的 inode/创建时间区分重连；无法辨认连接时不缓存。
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Ok(meta) = fs::metadata("/proc/self/fd/0") {
            fingerprint.push_str(&format!(
                "{}:{}:{}",
                meta.ino(),
                meta.ctime(),
                meta.ctime_nsec()
            ));
        }
    }
    if std::env::var_os("TMUX").is_some() {
        let client = std::process::Command::new("tmux")
            .args(["display-message", "-p", "#{client_tty}:#{client_pid}"])
            .output()?;
        fingerprint.push_str(&String::from_utf8_lossy(&client.stdout));
    }
    let path = project
        .root
        .join("config/.cache/clipboard")
        .join(format!("{:x}", Sha256::digest(fingerprint.as_bytes())));
    project.contained(&path)?;
    let mut status = if force {
        String::new()
    } else {
        fs::read_to_string(&path).unwrap_or_default()
    };
    if !matches!(status.as_str(), "supported" | "unavailable" | "unknown") {
        eprintln!("终端剪贴板自测：测试会覆盖当前剪贴板。");
        let mut random = [0; 8];
        rand::rng().fill_bytes(&mut random);
        let marker = format!(
            "ID-CLIP-{}",
            random
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        osc(&STANDARD.encode(marker.as_bytes()))?;
        let clear = ClearClipboard;
        let answer = ui::secret("粘贴测试文字并回车；无法粘贴直接回车，q 跳过: ");
        status = match answer {
            Ok(value) if value.bytes() == marker.as_bytes() => "supported",
            Ok(value) if value.bytes() == b"q" => "unknown",
            Ok(_) => "unavailable",
            Err(_) => "unknown",
        }
        .into();
        drop(clear);
        crate::private_dir(path.parent().unwrap())?;
        fs::write(path, &status)?;
    }
    if status != "supported" {
        eprintln!(
            "{}",
            ui::color(
                "全局提示：本次连接的剪贴板未验证可用；可在设置允许明文查看，或运行 clipboard-check 重测。",
                "33"
            )
        );
    }
    Ok(status)
}

fn field_path(
    project: &Project,
    identity: &str,
    title: &str,
    field: &str,
    session: &Session,
) -> Result<String> {
    let path = store::resolve_path(project, identity, title, session)?
        .ok_or_else(|| anyhow::anyhow!("条目不存在"))?;
    let items = store::Store::open(project, identity, session)?.items(identity)?;
    ensure!(
        items
            .iter()
            .any(|i| i.path == path && i.fields.iter().any(|f| f.name == field)),
        "字段不存在，请核对名称"
    );
    Ok(path)
}
fn select_entry(
    project: &Project,
    identity: &str,
    query: &str,
    field: &str,
    session: &Session,
) -> Result<Option<String>> {
    if field == "Password" {
        return store::select_password(project, identity, query, session);
    }
    let paths = store::Store::open(project, identity, session)?
        .items(identity)?
        .into_iter()
        .filter(|i| i.fields.iter().any(|f| f.name == field))
        .map(|i| i.path)
        .collect::<Vec<_>>();
    ensure!(!paths.is_empty(), "没有包含此字段的条目");
    Ok(ui::choose("选择字段所在条目", &paths)?.map(|i| paths[i].clone()))
}
fn attribute(
    project: &Project,
    identity: &str,
    title: &str,
    field: &str,
    session: &Session,
) -> Result<Zeroizing<Vec<u8>>> {
    let vault = identity::vault(project, identity)?;
    let resolved;
    let title = if field == "Password" {
        title
    } else {
        resolved = field_path(project, identity, title, field, session)?;
        &resolved
    };
    let mut result = match session.run(
        &vault,
        "show",
        &["-s".into(), "-a".into(), field.into(), title.into()],
        &[],
    ) {
        Ok(output) => output,
        Err(error) if field != "Password" => return Err(error),
        Err(_) => {
            let path = match store::resolve_path(project, identity, title, session)? {
                Some(path) => Some(path),
                None => select_entry(project, identity, title, field, session)?,
            }
            .ok_or_else(|| anyhow::anyhow!("已取消"))?;
            session.run(
                &vault,
                "show",
                &["-s".into(), "-a".into(), field.into(), path],
                &[],
            )?
        }
    };
    if result.last() == Some(&b'\n') {
        result.pop();
    }
    Ok(result)
}
fn wrapped(value: &str, width: usize) -> Vec<&str> {
    let mut lines = Vec::new();
    for paragraph in value.split('\n') {
        let (mut start, mut used) = (0, 0);
        for (i, c) in paragraph.char_indices() {
            let size = c.width().unwrap_or(0);
            if used + size > width.max(1) && i > start {
                lines.push(&paragraph[start..i]);
                start = i;
                used = 0;
            }
            used += size;
        }
        lines.push(&paragraph[start..]);
    }
    lines
}
fn reveal(project: &Project, field: &str, value: &str) -> Result<()> {
    let result = (|| -> Result<()> {
        let mut offset = 0usize;
        loop {
            config::ensure_plaintext(project)?;
            let (width, height) = terminal::size()?;
            let lines = wrapped(value, usize::from(width).saturating_sub(1).max(1));
            let page = usize::from(height).saturating_sub(4).max(1);
            offset = offset.min(lines.len().saturating_sub(page));
            let mut out = io::stdout().lock();
            execute!(out, cursor::MoveTo(0, 0), Clear(ClearType::All))?;
            write!(
                out,
                "{}\r\n\r\n",
                ui::color(&ui::clip(&format!("字段 / {field}"), width.into()), "1;36")
            )?;
            for line in lines.iter().skip(offset).take(page) {
                out.write_all(line.as_bytes())?;
                out.write_all(b"\r\n")?;
            }
            write!(
                out,
                "{}",
                ui::clip("↑↓ / PgUp/PgDn 滚动  Enter / Esc 隐藏", width.into())
            )?;
            out.flush()?;
            drop(out);
            if let Some(key) = ui::key(Duration::from_millis(200))? {
                if ui::cancelled(key) || matches!(key.code, KeyCode::Enter | KeyCode::Char('q')) {
                    return Ok(());
                }
                match key.code {
                    KeyCode::Down => offset += 1,
                    KeyCode::Up => offset = offset.saturating_sub(1),
                    KeyCode::PageDown => offset += page,
                    KeyCode::PageUp => offset = offset.saturating_sub(page),
                    _ => {}
                }
            }
        }
    })();
    let _ = execute!(io::stdout(), cursor::MoveTo(0, 0), Clear(ClearType::All));
    result
}
fn show(
    project: &Project,
    identity: &str,
    title: &str,
    field: &str,
    session: &Session,
) -> Result<()> {
    config::ensure_plaintext(project)?;
    eprintln!("内容将显示在当前终端，可能保留在终端记录中。");
    session.unlock(&identity::vault(project, identity)?, true)?;
    let title = if title.is_empty() {
        select_entry(project, identity, "", field, session)?
            .ok_or_else(|| anyhow::anyhow!("已取消"))?
    } else {
        title.to_owned()
    };
    ensure!(!title.chars().any(char::is_control), "条目标题无效");
    let value = attribute(project, identity, &title, field, session)?;
    config::ensure_plaintext(project)?;
    ensure!(
        !std::str::from_utf8(&value)?
            .chars()
            .any(|c| c.is_control() && !(field != "Password" && c == '\n')),
        "内容包含终端控制字符，请在 KeePassXC 图形界面查看"
    );
    if field != "Password" && ui::is_fullscreen() {
        return reveal(project, field, std::str::from_utf8(&value)?);
    }
    let mut out = io::stdout().lock();
    out.write_all(&value)?;
    out.write_all(b"\n")?;
    out.flush()?;
    drop(out);
    drop(value);
    if ui::is_fullscreen() {
        ui::input("按 Enter 返回 AI: ")?;
    }
    Ok(())
}
pub fn pass(
    project: &Project,
    identity: &str,
    title: Option<&str>,
    field: &str,
    plaintext: bool,
    session: &Session,
) -> Result<()> {
    ui::require_terminal()?;
    ensure!(
        !field.is_empty() && !field.chars().any(char::is_control),
        "字段名无效"
    );
    if plaintext {
        return show(project, identity, title.unwrap_or_default(), field, session);
    }
    let status = check(project, false)?;
    if field != "Password" && status != "unavailable" {
        session.unlock(&identity::vault(project, identity)?, true)?;
    }
    let title = if let Some(t) = title.filter(|v| !v.is_empty()) {
        t.to_owned()
    } else {
        select_entry(project, identity, "", field, session)?
            .ok_or_else(|| anyhow::anyhow!("已取消"))?
    };
    ensure!(!title.chars().any(char::is_control), "条目标题无效");
    if status == "unavailable" {
        config::ensure_plaintext(project)?;
        if ui::confirm(if field == "Password" {
            "剪贴板不可用，是否明文显示密码？"
        } else {
            "剪贴板不可用，是否明文显示字段？"
        })? {
            return show(project, identity, &title, field, session);
        }
        return Ok(());
    }
    if status == "native" {
        let vault = identity::vault(project, identity)?;
        let title = if field == "Password" {
            title
        } else {
            field_path(project, identity, &title, field, session)?
        };
        if session
            .run(
                &vault,
                "clip",
                &["-a".into(), field.into(), title.clone(), "10".into()],
                &[],
            )
            .is_err()
        {
            ensure!(field == "Password", "字段复制失败，请核对本机剪贴板");
            let path = match store::resolve_path(project, identity, &title, session)? {
                Some(path) => Some(path),
                None => select_entry(project, identity, &title, field, session)?,
            };
            if let Some(path) = path {
                session.run(
                    &vault,
                    "clip",
                    &["-a".into(), field.into(), path, "10".into()],
                    &[],
                )?;
            }
        }
        return Ok(());
    }
    {
        let secret = attribute(project, identity, &title, field, session)?;
        let encoded = Zeroizing::new(STANDARD.encode(&*secret));
        osc(&encoded)?;
    }
    let clear = ClearClipboard;
    let mut fallback = false;
    {
        let _terminal = ui::Terminal::new(false)?;
        print!("已发送复制请求，请在 10 秒内粘贴；终端接收结果无法保证。\r\n");
        if config::Config::plaintext_allowed(project) {
            print!("输入 v 再按 Enter 可申请明文显示；Enter 结束。\r\n");
        }
        io::stdout().flush()?;
        let until = Instant::now() + Duration::from_secs(10);
        let mut request = false;
        while Instant::now() < until {
            if let Some(k) = ui::key(Duration::from_millis(100))? {
                if ui::cancelled(k) {
                    break;
                }
                match k.code {
                    KeyCode::Char('v') => request = true,
                    KeyCode::Enter => {
                        fallback = request;
                        break;
                    }
                    _ => {}
                }
            }
        }
    }
    drop(clear);
    if fallback {
        show(project, identity, &title, field, session)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_view_wraps_unicode_and_preserves_empty_lines_without_copying_values() {
        let value = "00123\n中文甲乙\n\nEND";
        assert_eq!(wrapped(value, 4), ["0012", "3", "中文", "甲乙", "", "END"]);
        assert_eq!(wrapped("中a", 1), ["中", "a"]);
    }
}

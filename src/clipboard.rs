use crate::{Project, config, identity, store, ui, vault::Session};
use anyhow::{Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use crossterm::event::KeyCode;
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Write},
    time::{Duration, Instant},
};
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

fn password(
    project: &Project,
    identity: &str,
    title: &str,
    session: &Session,
) -> Result<Zeroizing<Vec<u8>>> {
    let vault = identity::vault(project, identity)?;
    let mut result = match session.run(
        &vault,
        "show",
        &["-s".into(), "-a".into(), "Password".into(), title.into()],
        &[],
    ) {
        Ok(output) => output,
        Err(_) => {
            let path = store::select_password(project, identity, title, session)?
                .ok_or_else(|| anyhow::anyhow!("已取消"))?;
            session.run(
                &vault,
                "show",
                &["-s".into(), "-a".into(), "Password".into(), path],
                &[],
            )?
        }
    };
    if result.last() == Some(&b'\n') {
        result.pop();
    }
    Ok(result)
}
fn show(project: &Project, identity: &str, title: &str, session: &Session) -> Result<()> {
    config::ensure_plaintext(project)?;
    eprintln!("密码将显示在当前终端，可能保留在滚屏记录中。");
    session.unlock(&identity::vault(project, identity)?, true)?;
    let value = password(project, identity, title, session)?;
    config::ensure_plaintext(project)?;
    ensure!(
        !std::str::from_utf8(&value)?.chars().any(char::is_control),
        "密码包含控制字符，请在 KeePassXC 图形界面查看"
    );
    let mut out = io::stdout().lock();
    out.write_all(&value)?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}
pub fn pass(
    project: &Project,
    identity: &str,
    title: Option<&str>,
    plaintext: bool,
    session: &Session,
) -> Result<()> {
    ui::require_terminal()?;
    if plaintext {
        config::ensure_plaintext(project)?;
    }
    let status = if plaintext {
        String::new()
    } else {
        check(project, false)?
    };
    let title = if let Some(t) = title.filter(|v| !v.is_empty()) {
        t.to_owned()
    } else {
        store::select_password(project, identity, "", session)?
            .ok_or_else(|| anyhow::anyhow!("已取消"))?
    };
    ensure!(!title.chars().any(char::is_control), "条目标题无效");
    if plaintext {
        return show(project, identity, &title, session);
    }
    if status == "unavailable" {
        config::ensure_plaintext(project)?;
        if ui::confirm("剪贴板不可用，是否明文显示密码？")? {
            return show(project, identity, &title, session);
        }
        return Ok(());
    }
    if status == "native" {
        let vault = identity::vault(project, identity)?;
        if session
            .run(&vault, "clip", &[title.clone(), "10".into()], &[])
            .is_err()
        {
            if let Some(title) = store::select_password(project, identity, &title, session)? {
                session.run(&vault, "clip", &[title, "10".into()], &[])?;
            }
        }
        return Ok(());
    }
    {
        let secret = password(project, identity, &title, session)?;
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
        show(project, identity, &title, session)?;
    }
    Ok(())
}

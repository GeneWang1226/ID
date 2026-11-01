//! 临时凭证的本地签发与代执行。不得让模型读取凭证目录。
use crate::{Project, identity, maintenance, private_dir, private_file, ui, vault::Session};
use anyhow::{Context, Result, ensure};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};
use zeroize::Zeroizing;

#[derive(Serialize, Deserialize)]
struct Meta {
    identity: String,
    entry: String,
    expires: u64,
    oneshot: bool,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn token() -> String {
    let mut random = [0; 16];
    rand::rng().fill_bytes(&mut random);
    random.iter().map(|b| format!("{b:02x}")).collect()
}
fn valid_token(token: &str) -> Result<()> {
    ensure!(
        token.len() == 32
            && token
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
        "凭证格式无效"
    );
    Ok(())
}
fn directory() -> Result<PathBuf> {
    #[cfg(unix)]
    {
        let uid = rustix::process::getuid().as_raw();
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(format!("/run/user/{uid}")));
        ensure!(
            runtime.is_dir() && !runtime.is_symlink(),
            "需要当前用户的 XDG_RUNTIME_DIR 运行目录"
        );
        #[cfg(target_os = "linux")]
        {
            let runtime = runtime.canonicalize()?;
            let mounts = fs::read_to_string("/proc/mounts")?;
            let mut selected = (0usize, "");
            for line in mounts.lines() {
                let fields = line.split_whitespace().collect::<Vec<_>>();
                if fields.len() > 2
                    && runtime.starts_with(fields[1])
                    && fields[1].len() > selected.0
                {
                    selected = (fields[1].len(), fields[2]);
                }
            }
            ensure!(selected.1 == "tmpfs", "临时凭证仅允许存放在 tmpfs，未签发");
        }
        use std::os::unix::fs::MetadataExt;
        ensure!(
            fs::metadata(&runtime)?.uid() == uid,
            "运行目录不属于当前用户"
        );
        let path = runtime.join("id-grants");
        ensure!(!path.is_symlink(), "凭证目录不能使用符号链接");
        private_dir(&path)?;
        ensure!(fs::metadata(&path)?.uid() == uid, "凭证目录不属于当前用户");
        Ok(path)
    }
    #[cfg(not(unix))]
    {
        anyhow::bail!("临时凭证目前需要 Unix 私有运行目录")
    }
}
fn read_meta(path: &Path) -> Result<Meta> {
    ensure!(!path.is_symlink(), "凭证文件不能使用符号链接");
    let bytes = fs::read(path).context("凭证不存在或已被使用")?;
    if let Ok(meta) = serde_json::from_slice(&bytes) {
        return Ok(meta);
    }
    // 兼容旧 Bash 版本签发的元数据；不会执行文件内容。
    let values = std::str::from_utf8(&bytes)?
        .lines()
        .filter_map(|s| s.split_once('='))
        .collect::<std::collections::HashMap<_, _>>();
    Ok(Meta {
        identity: values.get("identity").context("凭证无效")?.to_string(),
        entry: values.get("entry").context("凭证无效")?.to_string(),
        expires: values.get("expires").context("凭证无效")?.parse()?,
        oneshot: values.get("oneshot") == Some(&"1"),
    })
}
fn remove(folder: &Path, token: &str) -> Result<()> {
    valid_token(token)?;
    for ext in ["meta", "secret"] {
        let p = folder.join(format!("{token}.{ext}"));
        if p.exists() {
            fs::remove_file(p)?;
        }
    }
    Ok(())
}
fn cleanup_expired(folder: &Path) -> Result<()> {
    for entry in fs::read_dir(folder)? {
        let path = entry?.path();
        if path.extension().is_some_and(|s| s == "meta") {
            let token = path.file_stem().unwrap().to_string_lossy();
            if valid_token(&token).is_ok() && read_meta(&path).is_ok_and(|m| m.expires <= now()) {
                remove(folder, &token)?;
            }
        }
    }
    Ok(())
}
pub fn grant(
    project: &Project,
    identity: &str,
    title: &str,
    ttl: Option<u64>,
    session: &Session,
) -> Result<()> {
    ui::require_terminal()?;
    ensure!(!title.chars().any(char::is_control), "条目标题无效");
    let folder = directory()?;
    cleanup_expired(&folder)?;
    let vault = identity::vault(project, identity)?;
    let title =
        crate::store::resolve_path(project, identity, title, session)?.context("条目不存在")?;
    let tags = session.run(
        &vault,
        "show",
        &["-a".into(), "Tags".into(), title.clone()],
        &[],
    )?;
    let tags = std::str::from_utf8(&tags)?
        .split([';', ','])
        .map(|s| s.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();
    ensure!(
        !tags.iter().any(|s| s == "ai-deny"),
        "条目标记为 ai-deny，拒绝签发"
    );
    let oneshot = !tags.iter().any(|s| s == "ai-auto");
    let duration = ttl.unwrap_or(if oneshot { 300 } else { 600 }).min(3600);
    ensure!(duration > 0, "TTL 必须大于零");
    let mut password = session.run(
        &vault,
        "show",
        &["-s".into(), "-a".into(), "Password".into(), title.clone()],
        &[],
    )?;
    if password.last() == Some(&b'\n') {
        password.pop();
    }
    ensure!(!password.is_empty(), "该条目没有密码");
    let token = token();
    let secret_path = folder.join(format!("{token}.secret"));
    let mut file = private_file(&secret_path)?;
    file.write_all(&password)?;
    file.sync_all()?;
    let meta = Meta {
        identity: identity.into(),
        entry: title,
        expires: now() + duration,
        oneshot,
    };
    if let Err(error) = (|| -> Result<()> {
        let mut file = private_file(&folder.join(format!("{token}.meta")))?;
        file.write_all(&serde_json::to_vec(&meta)?)?;
        file.sync_all()?;
        Ok(())
    })() {
        let _ = fs::remove_file(secret_path);
        return Err(error);
    }
    maintenance::log(
        project,
        &format!("grant {identity} ttl={duration} oneshot={oneshot}"),
    )?;
    println!(
        "已签发{}凭证，有效期 {duration} 秒。\ntoken: {token}",
        if oneshot { "一次性" } else { "限时" }
    );
    Ok(())
}
pub fn list() -> Result<()> {
    let folder = directory()?;
    println!("{}", ui::color("活跃临时凭证", "1;36"));
    for entry in fs::read_dir(&folder)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_none_or(|v| v != "meta") {
            continue;
        }
        let token = path.file_stem().unwrap().to_string_lossy();
        if valid_token(&token).is_err() {
            continue;
        }
        let meta = read_meta(&path)?;
        if meta.expires <= now() {
            remove(&folder, &token)?;
            continue;
        }
        println!(
            "{}  {} / {}  {}  剩余 {} 秒",
            token,
            ui::clean(&meta.identity),
            ui::clean(&meta.entry),
            if meta.oneshot { "一次性" } else { "多次" },
            meta.expires - now()
        );
    }
    Ok(())
}
pub fn revoke(project: &Project, token: Option<&str>, all: bool) -> Result<()> {
    let folder = directory()?;
    if all {
        for entry in fs::read_dir(&folder)? {
            let path = entry?.path();
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                && valid_token(stem).is_ok()
            {
                remove(&folder, stem)?;
            }
        }
    } else {
        remove(&folder, token.context("需要 token 或 --all")?)?;
    }
    maintenance::log(project, "grant revoked")?;
    println!("凭证已撤销。");
    Ok(())
}

fn redact(bytes: &[u8], secret: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    let mut rest = bytes;
    while !rest.is_empty() {
        if rest.starts_with(secret) {
            result.extend_from_slice(b"***");
            rest = &rest[secret.len()..];
        } else {
            result.push(rest[0]);
            rest = &rest[1..];
        }
    }
    result
}
pub fn use_grant(project: &Project, token: &str, command: &str) -> Result<i32> {
    valid_token(token)?;
    let folder = directory()?;
    let path = folder.join(format!("{token}.meta"));
    let meta = read_meta(&path)?;
    if meta.expires <= now() {
        remove(&folder, token)?;
        anyhow::bail!("凭证已过期");
    }
    let secret_path = folder.join(format!("{token}.secret"));
    ensure!(!secret_path.is_symlink(), "凭证文件不能使用符号链接");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        ensure!(
            fs::metadata(&secret_path)?.permissions().mode() & 0o077 == 0,
            "凭证文件权限无效"
        );
    }
    let claim = folder.join(format!("{token}.claim-{}", std::process::id()));
    // 一次性凭证先原子领取，再读秘密；并发调用只能有一个成功。
    if meta.oneshot {
        fs::rename(&path, &claim).context("凭证已被其他操作使用")?;
    }
    let secret = Zeroizing::new(fs::read(&secret_path)?);
    ensure!(!secret.is_empty(), "凭证为空");
    if meta.oneshot {
        fs::remove_file(&secret_path)?;
        fs::remove_file(&claim)?;
    }
    let mut child = Command::new("bash")
        .args(["-c", command])
        .env("ID_SECRET", std::str::from_utf8(&secret)?)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("无法启动代执行命令")?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let read = |stream: Box<dyn Read + Send>| {
        std::thread::spawn(move || -> std::io::Result<Zeroizing<Vec<u8>>> {
            let mut output = Zeroizing::new(Vec::with_capacity(16 * 1024 * 1024 + 1));
            stream.take(16 * 1024 * 1024 + 1).read_to_end(&mut output)?;
            Ok(output)
        })
    };
    let out = read(Box::new(stdout));
    let err = read(Box::new(stderr));
    let status = child.wait()?;
    let out = out.join().map_err(|_| anyhow::anyhow!("读取输出失败"))??;
    let err = err.join().map_err(|_| anyhow::anyhow!("读取输出失败"))??;
    ensure!(
        out.len() <= 16 * 1024 * 1024 && err.len() <= 16 * 1024 * 1024,
        "命令输出超过限制，未展示内容"
    );
    std::io::stdout().write_all(&redact(&out, &secret))?;
    std::io::stderr().write_all(&redact(&err, &secret))?;
    maintenance::log(project, "grant command executed")?;
    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redaction_treats_secret_as_literal_bytes() {
        assert_eq!(redact(b"abc A*[x] A*[x]", b"A*[x]"), b"abc *** ***");
        assert!(valid_token("../foo").is_err());
        assert!(valid_token("0123456789abcdef0123456789abcdef").is_ok());
    }
}

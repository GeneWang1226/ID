use crate::{Project, ui};
use anyhow::{Context, Result, ensure};
use std::{collections::HashSet, fs, path::Path, process::Command};

fn forbidden(path: &str) -> bool {
    ["identities/", "backups/", "logs/", "exports/"]
        .iter()
        .any(|p| path.starts_with(p))
        || path.ends_with(".kdbx")
        || path.ends_with(".turso.env")
        || matches!(
            path,
            "docker/.env" | "config/secrets/.secrets.env" | ".env" | ".secrets.env"
        )
        || (path.starts_with("data/") && (path.ends_with(".db") || path.contains(".db-")))
}
fn rules(path: &Path) -> Result<HashSet<String>> {
    let mut lines = HashSet::new();
    for line in fs::read_to_string(path)?.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        ensure!(lines.insert(line.to_owned()), "忽略文件中存在重复规则");
    }
    Ok(lines)
}
#[cfg(unix)]
fn permission(path: &Path, expected: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if path.exists() {
        ensure!(
            fs::metadata(path)?.permissions().mode() & 0o777 == expected,
            "权限不符合要求：{}（应为 {expected:o}）",
            ui::clean(&path.display().to_string())
        );
    }
    Ok(())
}
pub fn check(project: &Project) -> Result<()> {
    rules(&project.root.join(".gitignore"))?;
    let sync = rules(&project.root.join(".stignore"))?;
    for rule in [
        "/src",
        "/tests",
        "/target",
        "/Cargo.toml",
        "/Cargo.lock",
        "/template",
        "/assets",
        "/docs",
        "/web",
        "/docker",
        "/config",
        "/Makefile",
        "/AGENTS.md",
        "/README.md",
        "/.gitignore",
        ".git",
        "/logs",
        "/backups",
        "/data",
        "/exports",
        ".id-*",
        ".turso.env",
    ] {
        ensure!(
            sync.iter()
                .any(|s| s.trim_start_matches("(?d)").trim_start_matches("(?i)") == rule),
            ".stignore 缺少 {rule}"
        );
    }
    let tracked = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(&project.root)
        .output()?;
    ensure!(tracked.status.success(), "无法检查 Git 追踪范围");
    let paths = tracked
        .stdout
        .split(|&b| b == 0)
        .filter(|p| !p.is_empty())
        .map(std::str::from_utf8)
        .collect::<Result<Vec<_>, _>>()?;
    ensure!(
        !paths.iter().any(|p| forbidden(p)),
        "Git 正在追踪禁止入库的路径"
    );
    for path in [
        "identities",
        "backups",
        "logs",
        "data/id.db",
        "data/id.db-wal",
        "data/id.db-shm",
        "docker/.env",
        "config/secrets/.secrets.env",
        "target",
        "check.kdbx",
    ] {
        ensure!(
            Command::new("git")
                .args(["check-ignore", "-q", path])
                .current_dir(&project.root)
                .status()?
                .success(),
            "忽略规则缺少 {path}"
        );
    }
    for path in [
        "docker/.env.example",
        "config/secrets/.secrets.env.example",
        "config/config.toml",
    ] {
        ensure!(project.root.join(path).is_file(), "配置模板缺少 {path}");
    }
    #[cfg(unix)]
    {
        for path in ["docker/.env", "config/secrets/.secrets.env"] {
            permission(&project.root.join(path), 0o600)?;
        }
        permission(&project.root.join("logs"), 0o700)?;
        fn identity_permissions(path: &Path) -> Result<()> {
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                let path = entry.path();
                ensure!(!entry.file_type()?.is_symlink(), "身份文件不能使用符号链接");
                if path.is_dir() {
                    permission(&path, 0o700)?;
                    identity_permissions(&path)?;
                } else if path.extension().is_some_and(|s| s == "kdbx")
                    || path.file_name().is_some_and(|s| s == ".turso.env")
                {
                    permission(&path, 0o600)?;
                }
            }
            Ok(())
        }
        let identities = project.root.join("identities");
        if identities.exists() {
            permission(&identities, 0o700)?;
            identity_permissions(&identities)?;
        }
    }
    // 只扫描已追踪的项目文件，不输出匹配行或潜在秘密。
    let scan = Command::new("git")
        .args([
            "grep",
            "-lE",
            "BEGIN [A-Z ]*PRIVATE KEY|api_key=sk-[A-Za-z0-9]|AUTH_TOKEN=ey[A-Za-z0-9]",
            "--",
            ".",
        ])
        .current_dir(&project.root)
        .output()
        .context("无法扫描追踪文件")?;
    ensure!(
        scan.status.code() == Some(1),
        "追踪文件扫描失败或发现疑似机密，请在本地检查"
    );
    println!("安全检查通过。");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sensitive_paths_and_duplicate_rules_are_rejected() {
        for p in [
            "identities/a/profile.md",
            "data/id.db-wal",
            "backups/test.xlsx",
            "x/password.kdbx",
            "docker/.env",
        ] {
            assert!(forbidden(p));
        }
        for p in ["config/config.toml", "docker/.env.example", "Cargo.lock"] {
            assert!(!forbidden(p));
        }
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("ignore");
        fs::write(&f, "# skip\n/src\n/src\n").unwrap();
        assert!(rules(&f).is_err());
    }
}

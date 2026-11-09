//! Inspect filenames and metadata only; repairs move opaque copies out of the live folder.
use crate::{Project, identity, kdbx, maintenance, private_dir, ui, valid_name};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Conflict,
    Backup,
    Temporary,
    Lock,
    Empty,
    Duplicate,
    Symlink,
    Permissions,
    Unknown,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Self::Conflict => "同步冲突副本",
            Self::Backup => "遗留备份",
            Self::Temporary => "临时文件",
            Self::Lock => "外部锁文件",
            Self::Empty => "空密码库",
            Self::Duplicate => "新旧库并存",
            Self::Symlink => "符号链接",
            Self::Permissions => "目录权限异常",
            Self::Unknown => "未识别文件或目录",
        }
    }
    fn advice(self) -> &'static str {
        match self {
            Self::Conflict => "先在 KeePassXC 合并核对；也可确认后保留加密归档，正式库不变",
            Self::Backup => "可确认后移入本机加密归档",
            Self::Temporary => "可能正在同步或保存，保留文件；检查 Syncthing 的错误和进度",
            Self::Lock => "先退出相关编辑器；不按文件年龄判断锁是否失效",
            Self::Empty => "保留原文件，从加密备份人工核对恢复",
            Self::Duplicate => "人工核对新旧密码库，不自动选择或覆盖",
            Self::Symlink => "不跟随链接，人工核对真实存储路径",
            Self::Permissions => {
                "核对目录访问权限、NAS ACL 和 Syncthing 权限同步设置，不自动 chmod"
            }
            Self::Unknown => "保留原文件，人工判断用途；旧身份资料不自动搬动",
        }
    }
    fn archivable(self) -> bool {
        matches!(self, Self::Conflict | Self::Backup)
    }
}

#[derive(Clone)]
struct Issue {
    path: PathBuf,
    owner: Option<String>,
    kind: Kind,
}

pub struct Report {
    issues: Vec<Issue>,
}

impl Report {
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }
    pub fn issue_count(&self) -> usize {
        self.issues.len()
    }
    pub fn lines(&self, project: &Project) -> Vec<String> {
        if self.is_clean() {
            return vec!["identities 目录正常。".into()];
        }
        let mut lines = vec![format!("identities · {} 项待处理", self.issue_count())];
        for issue in &self.issues {
            let path = issue
                .path
                .strip_prefix(&project.root)
                .unwrap_or(&issue.path);
            lines.push(format!(
                "{}  {}",
                issue.kind.label(),
                ui::clean(&path.display().to_string())
            ));
            lines.push(format!("  {}", issue.kind.advice()));
        }
        lines
    }
    // File names and paths can themselves contain private data; the model receives counts only.
    pub fn ai_data(&self) -> Value {
        let mut counts = BTreeMap::new();
        for issue in &self.issues {
            *counts.entry(issue.kind.label()).or_insert(0usize) += 1;
        }
        json!({"clean":self.is_clean(),"issue_count":self.issue_count(),"issues":counts,
            "repair_action":"directory-repair",
            "policy":"本机目录检查；不含文件内容、文件名或路径。归档需本地确认且保留正式库；临时文件、锁、空库、链接、权限和未知文件仅诊断。加密归档不代表已合并冲突。"})
    }
}

fn classify(name: &str) -> (Option<&str>, Option<Kind>) {
    if let Some(rest) = name.strip_prefix(".syncthing.")
        && let Some(owner) = rest.strip_suffix(".kdbx.tmp")
        && valid_name(owner).is_ok()
    {
        return (Some(owner), Some(Kind::Temporary));
    }
    if let Some((owner, tail)) = name.split_once(".sync-conflict-")
        && valid_name(owner).is_ok()
        && tail.ends_with(".kdbx")
    {
        return (Some(owner), Some(Kind::Conflict));
    }
    for (suffix, kind) in [
        (".kdbx.lock", Kind::Lock),
        (".kdbx.tmp", Kind::Temporary),
        (".kdbx.bak", Kind::Backup),
        (".kdbx.old", Kind::Backup),
        (".old.kdbx", Kind::Backup),
        (".kdbx~", Kind::Backup),
    ] {
        if let Some(owner) = name.strip_suffix(suffix)
            && valid_name(owner).is_ok()
        {
            return (Some(owner), Some(kind));
        }
    }
    if let Some(owner) = name.strip_suffix(".kdbx")
        && valid_name(owner).is_ok()
    {
        return (Some(owner), None);
    }
    (None, Some(Kind::Unknown))
}

fn inspect(
    project: &Project,
    folder: &Path,
    scope: Option<&str>,
    legacy: bool,
    report: &mut Report,
) -> Result<()> {
    let meta = fs::symlink_metadata(folder)?;
    if meta.is_symlink() {
        report.issues.push(Issue {
            path: folder.into(),
            owner: scope.map(str::to_owned),
            kind: Kind::Symlink,
        });
        return Ok(());
    }
    project.contained(folder)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o700 != 0o700 {
            report.issues.push(Issue {
                path: folder.into(),
                owner: scope.map(str::to_owned),
                kind: Kind::Permissions,
            });
        }
    }
    for entry in fs::read_dir(folder).context("无法列出身份目录，请检查目录权限或 NAS ACL")?
    {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        let ty = entry.file_type()?;
        if !legacy && ty.is_dir() && valid_name(&name).is_ok() {
            if scope.is_some_and(|s| s != name) {
                continue;
            }
            project.contained(&path)?;
            let passwords = path.join("passwords");
            if passwords.is_symlink() {
                report.issues.push(Issue {
                    path: passwords,
                    owner: Some(name.into_owned()),
                    kind: Kind::Symlink,
                });
            } else if passwords.join(format!("{name}.kdbx")).is_file() {
                if folder.join(format!("{name}.kdbx")).exists() {
                    report.issues.push(Issue {
                        path: path.clone(),
                        owner: Some(name.to_string()),
                        kind: Kind::Duplicate,
                    });
                }
                inspect(project, &passwords, Some(&name), true, report)?;
            } else {
                report.issues.push(Issue {
                    path,
                    owner: Some(name.into_owned()),
                    kind: Kind::Unknown,
                });
            }
            continue;
        }
        let (owner, mut kind) = classify(&name);
        if scope.is_some_and(|s| owner != Some(s) && name != s) {
            continue;
        }
        if ty.is_symlink() {
            kind = Some(Kind::Symlink);
        } else if !ty.is_file() {
            kind = Some(Kind::Unknown);
        } else if kind.is_none() && entry.metadata()?.len() == 0 {
            kind = Some(Kind::Empty);
        }
        if let Some(kind) = kind {
            report.issues.push(Issue {
                path,
                owner: owner.map(str::to_owned),
                kind,
            });
        }
    }
    Ok(())
}

pub fn scan(project: &Project, scope: Option<&str>) -> Result<Report> {
    if let Some(name) = scope {
        valid_name(name)?;
    }
    let mut report = Report { issues: Vec::new() };
    let folder = project.root.join("identities");
    if folder.exists() || folder.is_symlink() {
        inspect(project, &folder, scope, false, &mut report)?;
    }
    report.issues.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(report)
}

pub(crate) fn ensure_no_conflicts(vault: &Path) -> Result<()> {
    let owner = vault
        .file_stem()
        .context("密码库名称无效")?
        .to_string_lossy();
    let folder = vault.parent().context("密码库路径无效")?;
    let legacy_root = (folder.file_name().is_some_and(|n| n == "passwords")
        && folder
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|n| n == owner.as_ref()))
    .then(|| folder.parent().and_then(Path::parent))
    .flatten();
    for folder in std::iter::once(folder).chain(legacy_root) {
        if !folder.exists() {
            continue;
        }
        for entry in fs::read_dir(folder)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let (id, kind) = classify(&name);
            ensure!(
                id != Some(owner.as_ref()) || kind != Some(Kind::Conflict),
                "存在此身份的同步冲突副本，请先运行 make doctor 或在 KeePassXC 合并核对；所有文件均保留"
            );
        }
    }
    Ok(())
}

fn archive(project: &Project, issue: &Issue, expected: [u8; 32]) -> Result<PathBuf> {
    ensure!(issue.kind.archivable(), "此类异常只允许诊断");
    let owner = issue.owner.as_deref().context("无法确认副本所属身份")?;
    let _lock = kdbx::WriteLock::for_repair(project, owner)?;
    let vault = identity::vault(project, owner)?;
    ensure!(fs::metadata(&vault)?.len() > 0, "正式库为空，副本保留原位");
    for entry in fs::read_dir(vault.parent().unwrap())? {
        let name = entry?.file_name();
        let name = name.to_string_lossy();
        let (id, kind) = classify(&name);
        ensure!(
            id != Some(owner) || !matches!(kind, Some(Kind::Lock | Kind::Temporary)),
            "存在临时文件或外部锁，请等同步完成并退出编辑器后重试；副本保留原位"
        );
    }
    project.contained(&issue.path)?;
    ensure!(
        fs::symlink_metadata(&issue.path)?.is_file(),
        "副本不是普通文件，未归档"
    );
    ensure!(
        issue.path.parent() == vault.parent(),
        "副本与正式库路径不一致，未归档"
    );
    ensure!(
        kdbx::fingerprint(&issue.path)? == expected,
        "副本已变化，请重新检查；原文件保留"
    );
    let folder = project.root.join("backups/identity-cleanup");
    project.contained(&folder)?;
    ensure!(
        fs::read_to_string(project.root.join(".stignore"))?
            .lines()
            .any(|l| l.trim() == "/backups"),
        "请先在 .stignore 排除 /backups，未归档"
    );
    private_dir(&folder)?;
    let archive = tempfile::Builder::new()
        .prefix(&format!("{owner}-"))
        .tempdir_in(&folder)?
        .keep();
    private_dir(&archive)?;
    let target = archive.join(issue.path.file_name().unwrap());
    // Rename preserves the entire original even if a non-ID editor races this operation.
    fs::rename(&issue.path, &target).context("归档失败，原文件保留；不跨文件系统复制后删除")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).with_context(|| {
            format!(
                "副本已归档到 {}，但私有权限设置失败",
                ui::clean(&target.display().to_string())
            )
        })?;
    }
    fs::File::open(&target)?.sync_all().with_context(|| {
        format!(
            "副本已归档到 {}，但落盘确认失败",
            ui::clean(&target.display().to_string())
        )
    })?;
    #[cfg(unix)]
    {
        fs::File::open(&archive)?.sync_all()?;
        fs::File::open(issue.path.parent().unwrap())?.sync_all()?;
    }
    ensure!(
        kdbx::fingerprint(&target)? == expected,
        "副本在归档期间发生变化，已完整保留在 {}；请人工核对",
        ui::clean(&target.display().to_string())
    );
    maintenance::log(project, &format!("identity anomaly archived {owner}"))?;
    Ok(target)
}

pub fn run(project: &Project, scope: Option<&str>, fix: bool) -> Result<()> {
    let report = scan(project, scope)?;
    for line in report.lines(project) {
        println!("{}", ui::clean(&line));
    }
    if fix && !report.is_clean() {
        let candidates: Vec<_> = report
            .issues
            .iter()
            .filter(|i| i.kind.archivable())
            .collect();
        if !candidates.is_empty() {
            let mut plan = Vec::new();
            for issue in candidates {
                project.contained(&issue.path)?;
                plan.push((issue, kdbx::fingerprint(&issue.path)?));
            }
            let mut choices = vec![
                "取消".into(),
                format!("归档全部 {} 个副本（保留正式库，不合并冲突）", plan.len()),
            ];
            for (issue, _) in &plan {
                choices.push(format!(
                    "仅归档：{}",
                    ui::clean(
                        &issue
                            .path
                            .strip_prefix(&project.root)?
                            .display()
                            .to_string()
                    )
                ));
            }
            let selection = ui::choose(
                "目录维护 · 先退出编辑器并等同步完成；加密归档保留全部内容",
                &choices,
            )?;
            let selected = selection.filter(|i| *i > 0).context("已取消")?;
            for (index, (issue, hash)) in plan.into_iter().enumerate() {
                if selected != 1 && selected != index + 2 {
                    continue;
                }
                let target = archive(project, issue, hash)?;
                println!(
                    "已归档：{}",
                    ui::clean(&target.strip_prefix(&project.root)?.display().to_string())
                );
            }
        }
    }
    let remaining = scan(project, scope)?;
    ensure!(
        remaining.is_clean(),
        "仍有 {} 项异常；运行 make doctor 查看，临时文件和未知资料均保留",
        remaining.issue_count()
    );
    if fix && !report.is_clean() {
        println!("identities 目录正常；加密归档不代表已合并冲突条目。");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, Project) {
        let dir = tempfile::tempdir().unwrap();
        let project = Project {
            root: dir.path().canonicalize().unwrap(),
        };
        fs::create_dir(project.root.join("identities")).unwrap();
        fs::write(project.root.join(".stignore"), "/backups\n").unwrap();
        fs::write(
            project.root.join("identities/alice.kdbx"),
            b"synthetic opaque current vault",
        )
        .unwrap();
        fs::write(
            project
                .root
                .join("identities/alice.sync-conflict-private-title.kdbx"),
            b"synthetic opaque conflicting version",
        )
        .unwrap();
        (dir, project)
    }

    fn conflict(project: &Project) -> Issue {
        scan(project, Some("alice"))
            .unwrap()
            .issues
            .into_iter()
            .find(|i| i.kind == Kind::Conflict)
            .unwrap()
    }

    #[test]
    fn diagnostics_are_read_only_scoped_and_never_send_paths_or_contents_to_ai() {
        let (_dir, project) = fixture();
        for (name, data) in [
            ("alice.kdbx.bak", b"backup".as_slice()),
            (".syncthing.alice.kdbx.tmp", b"ongoing transfer"),
            ("alice.kdbx.lock", b"active editor"),
            ("bob.kdbx", b""),
            ("unrelated.txt", b"PRIVATE-UNKNOWN-CONTENT"),
        ] {
            fs::write(project.root.join("identities").join(name), data).unwrap();
        }
        let report = scan(&project, Some("alice")).unwrap();
        assert_eq!(report.issue_count(), 4);
        let data = report.ai_data().to_string();
        for hidden in [
            "alice",
            "bob",
            "private-title",
            "PRIVATE-UNKNOWN-CONTENT",
            "identities/",
        ] {
            assert!(!data.contains(hidden));
        }
        assert_eq!(scan(&project, None).unwrap().issue_count(), 6);
        assert_eq!(
            fs::read(project.root.join("identities/.syncthing.alice.kdbx.tmp")).unwrap(),
            b"ongoing transfer"
        );
    }

    #[test]
    fn archive_preserves_both_versions_and_never_overwrites_previous_archives() {
        let (_dir, project) = fixture();
        let issue = conflict(&project);
        let original = fs::read(&issue.path).unwrap();
        let main = fs::read(project.root.join("identities/alice.kdbx")).unwrap();
        assert!(kdbx::WriteLock::acquire(&project, "alice").is_err());
        let first = archive(&project, &issue, kdbx::fingerprint(&issue.path).unwrap()).unwrap();
        assert_eq!(fs::read(&first).unwrap(), original);
        assert!(!issue.path.exists());
        fs::write(&issue.path, b"a second synthetic conflicting version").unwrap();
        let second = archive(&project, &issue, kdbx::fingerprint(&issue.path).unwrap()).unwrap();
        assert_ne!(first, second);
        assert_eq!(fs::read(&first).unwrap(), original);
        assert_eq!(
            fs::read(second).unwrap(),
            b"a second synthetic conflicting version"
        );
        assert_eq!(
            fs::read(project.root.join("identities/alice.kdbx")).unwrap(),
            main
        );
        assert!(kdbx::WriteLock::acquire(&project, "alice").is_ok());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(first).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn changed_busy_or_orphaned_copies_stay_in_place() {
        let (_dir, project) = fixture();
        let issue = conflict(&project);
        let hash = kdbx::fingerprint(&issue.path).unwrap();
        fs::write(&issue.path, b"changed after inspection").unwrap();
        assert!(
            archive(&project, &issue, hash)
                .unwrap_err()
                .to_string()
                .contains("副本已变化")
        );
        let hash = kdbx::fingerprint(&issue.path).unwrap();
        for name in ["alice.kdbx.lock", ".syncthing.alice.kdbx.tmp"] {
            let busy = project.root.join("identities").join(name);
            fs::write(&busy, b"synthetic busy marker").unwrap();
            assert!(
                archive(&project, &issue, hash)
                    .unwrap_err()
                    .to_string()
                    .contains("临时文件或外部锁")
            );
            assert!(issue.path.exists() && busy.exists());
            fs::remove_file(busy).unwrap();
        }
        let lock = kdbx::WriteLock::for_repair(&project, "alice").unwrap();
        assert!(archive(&project, &issue, hash).is_err());
        drop(lock);
        fs::remove_file(project.root.join("identities/alice.kdbx")).unwrap();
        assert!(archive(&project, &issue, hash).is_err());
        assert_eq!(fs::read(&issue.path).unwrap(), b"changed after inspection");
        assert!(ensure_no_conflicts(&project.root.join("identities/alice.kdbx")).is_err());
    }

    #[test]
    fn legacy_writes_also_check_conflicts_at_the_flat_location() {
        let (_dir, project) = fixture();
        let legacy = project.root.join("identities/alice/passwords/alice.kdbx");
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        fs::rename(project.root.join("identities/alice.kdbx"), &legacy).unwrap();
        assert!(ensure_no_conflicts(&legacy).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn links_and_ambiguous_legacy_layouts_are_never_repaired() {
        use std::os::unix::fs::symlink;
        let (_dir, project) = fixture();
        fs::create_dir_all(project.root.join("identities/alice/passwords")).unwrap();
        fs::write(
            project.root.join("identities/alice/passwords/alice.kdbx"),
            b"synthetic legacy",
        )
        .unwrap();
        let issue = conflict(&project);
        let hash = kdbx::fingerprint(&issue.path).unwrap();
        assert!(
            scan(&project, Some("alice"))
                .unwrap()
                .issues
                .iter()
                .any(|i| i.kind == Kind::Duplicate)
        );
        assert!(archive(&project, &issue, hash).is_err());
        fs::remove_file(project.root.join("identities/alice/passwords/alice.kdbx")).unwrap();
        let other = tempfile::tempdir().unwrap();
        symlink(other.path(), project.root.join("backups")).unwrap();
        assert!(archive(&project, &issue, hash).is_err());
        assert_eq!(fs::read_dir(other.path()).unwrap().count(), 0);
        assert!(issue.path.exists());
        symlink(other.path(), project.root.join("identities/bob.kdbx")).unwrap();
        assert!(
            scan(&project, Some("bob"))
                .unwrap()
                .issues
                .iter()
                .any(|i| i.kind == Kind::Symlink)
        );
    }
}

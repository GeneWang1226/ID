//! 主密码仅存于可擦除缓冲区，经匿名管道传入 KeePassXC；不进入参数、环境或错误信息。
use anyhow::{Context, Result, bail, ensure};
use std::{
    fmt,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use zeroize::{Zeroize, Zeroizing};

pub struct Secret(Zeroizing<Vec<u8>>);

impl Secret {
    pub fn empty() -> Self {
        Self(Zeroizing::new(Vec::with_capacity(4096)))
    }
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self(Zeroizing::new(bytes))
    }
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
    pub fn push(&mut self, value: &[u8]) -> Result<()> {
        ensure!(self.0.len() + value.len() <= 4096, "秘密输入过长");
        self.0.extend_from_slice(value);
        Ok(())
    }
    pub fn pop_char(&mut self) {
        let mut start = self.0.len();
        while start > 0 {
            start -= 1;
            if self.0[start] & 0xc0 != 0x80 {
                break;
            }
        }
        self.0[start..].zeroize();
        self.0.truncate(start);
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([REDACTED])")
    }
}

struct State {
    password: Option<Secret>,
    database: Option<PathBuf>,
    last_used: Instant,
    stop: bool,
}

pub struct Session {
    shared: Arc<(Mutex<State>, Condvar)>,
    worker: Option<JoinHandle<()>>,
    idle: Duration,
}

impl Default for Session {
    fn default() -> Self {
        Self::new(Duration::from_secs(300))
    }
}

impl Session {
    pub fn new(idle: Duration) -> Self {
        let shared = Arc::new((
            Mutex::new(State {
                password: None,
                database: None,
                last_used: Instant::now(),
                stop: false,
            }),
            Condvar::new(),
        ));
        let copy = shared.clone();
        let worker = thread::spawn(move || {
            let (lock, signal) = &*copy;
            let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
            while !state.stop {
                if state.password.is_some() && state.last_used.elapsed() >= idle {
                    state.password = None;
                    state.database = None;
                }
                let wait = if state.password.is_some() {
                    idle.saturating_sub(state.last_used.elapsed())
                } else {
                    idle
                };
                state = signal
                    .wait_timeout(state, wait)
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
            }
            state.password = None;
        });
        Self {
            shared,
            worker: Some(worker),
            idle,
        }
    }

    pub fn lock(&self) {
        let mut state = self.shared.0.lock().unwrap_or_else(|e| e.into_inner());
        state.password = None;
        state.database = None;
        self.shared.1.notify_all();
    }

    pub fn unlock(&self, database: &Path, fresh: bool) -> Result<()> {
        self.unlock_with(database, fresh, || {
            crate::ui::secret(if fresh {
                "再次验证主密码: "
            } else {
                "主密码: "
            })
        })
    }

    fn unlock_with(
        &self,
        database: &Path,
        fresh: bool,
        prompt: impl FnOnce() -> Result<Secret>,
    ) -> Result<()> {
        let database = database.canonicalize().context("密码库不存在")?;
        let mut state = self.shared.0.lock().unwrap_or_else(|e| e.into_inner());
        if fresh
            || state.database.as_ref() != Some(&database)
            || state.last_used.elapsed() >= self.idle
        {
            state.password = None;
            state.database = None;
        }
        if state.password.is_none() {
            let password = prompt()?;
            ensure!(
                !password.bytes().contains(&b'\n') && !password.bytes().contains(&b'\r'),
                "主密码不能包含换行"
            );
            run_with(&database, "ls", &[], &password, &[]).context("密码库解锁失败")?;
            state.password = Some(password);
            state.database = Some(database);
        }
        state.last_used = Instant::now();
        self.shared.1.notify_all();
        Ok(())
    }

    pub fn run(
        &self,
        database: &Path,
        operation: &str,
        args: &[String],
        answers: &[&Secret],
    ) -> Result<Zeroizing<Vec<u8>>> {
        self.unlock(database, false)?;
        let mut state = self.shared.0.lock().unwrap_or_else(|e| e.into_inner());
        let result = run_with(
            database,
            operation,
            args,
            state.password.as_ref().context("会话已锁定，请重试")?,
            answers,
        );
        state.last_used = Instant::now();
        self.shared.1.notify_all();
        result
    }

    pub fn merge_xml(
        &self,
        database: &Path,
        xml: &Path,
        patch: &Path,
        staged: &Path,
        deletions: &[String],
        attachments: &[(String, String, [u8; 32])],
    ) -> Result<Zeroizing<Vec<u8>>> {
        self.unlock(database, false)?;
        let mut state = self.shared.0.lock().unwrap_or_else(|e| e.into_inner());
        let password = state.password.as_ref().context("会话已锁定，请重试")?;
        run_with(
            xml,
            "import",
            &[patch.to_string_lossy().into_owned(), "-p".into()],
            password,
            &[password],
        )?;
        run_with(
            staged,
            "merge",
            &["-s".into(), patch.to_string_lossy().into_owned()],
            password,
            &[],
        )?;
        for path in deletions {
            run_with(staged, "rm", std::slice::from_ref(path), password, &[])?;
        }
        use sha2::Digest;
        for (path, name, expected) in attachments {
            let bytes = run_with(
                staged,
                "attachment-export",
                &["--stdout".into(), path.clone(), name.clone(), "-".into()],
                password,
                &[],
            )?;
            let hash: [u8; 32] = sha2::Sha256::digest(&*bytes).into();
            ensure!(&hash == expected, "附件保存校验失败，原密码库保留");
        }
        let result = run_with(staged, "export", &[], password, &[]);
        state.last_used = Instant::now();
        self.shared.1.notify_all();
        result
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        {
            let mut state = self.shared.0.lock().unwrap_or_else(|e| e.into_inner());
            state.stop = true;
            state.password = None;
        }
        self.shared.1.notify_all();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub fn run_with(
    database: &Path,
    operation: &str,
    args: &[String],
    password: &Secret,
    answers: &[&Secret],
) -> Result<Zeroizing<Vec<u8>>> {
    let mut child = Command::new("keepassxc-cli")
        .arg(operation)
        .arg("-q")
        .arg(database)
        .args(args)
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("无法启动 KeePassXC，请安装 keepassxc-cli")?;
    let stdout = child.stdout.take().context("无法建立密码库输出管道")?;
    fn read_bounded(
        input: impl Read + Send + 'static,
        limit: usize,
    ) -> JoinHandle<std::io::Result<Zeroizing<Vec<u8>>>> {
        thread::spawn(move || {
            // 固定容量避免敏感输出随 Vec 扩容遗留副本。
            let mut result = Zeroizing::new(Vec::with_capacity(limit + 1));
            input.take(limit as u64 + 1).read_to_end(&mut result)?;
            Ok(result)
        })
    }
    let limit = if matches!(operation, "export" | "attachment-export") {
        64 * 1024 * 1024
    } else {
        1024 * 1024
    };
    let reader = read_bounded(stdout, limit);
    let errors = read_bounded(
        child.stderr.take().context("无法建立密码库错误管道")?,
        16384,
    );
    let written = (|| -> std::io::Result<()> {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("stdin unavailable"))?;
        stdin.write_all(password.bytes())?;
        stdin.write_all(b"\n")?;
        for answer in answers {
            stdin.write_all(answer.bytes())?;
            stdin.write_all(b"\n")?;
        }
        Ok(())
    })();
    let status = child.wait();
    if status.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let output = reader
        .join()
        .map_err(|_| anyhow::anyhow!("读取密码库失败"))??;
    let errors = errors
        .join()
        .map_err(|_| anyhow::anyhow!("读取密码库状态失败"))??;
    let status = status?;
    let no_results = operation == "search"
        && status.code() == Some(1)
        && output.is_empty()
        && errors.as_slice() == b"No results for that search term.\n";
    if written.is_err() || (!status.success() && !no_results) {
        bail!("KeePassXC 操作失败，请核对主密码、条目和权限");
    }
    ensure!(
        output.len() <= limit && errors.len() <= 16384,
        "密码库输出过大"
    );
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secret_debug_is_redacted_and_input_is_bounded() {
        let mut s = Secret::from_bytes(b"synthetic-only".to_vec());
        assert!(!format!("{s:?}").contains("synthetic-only"));
        assert!(s.push(&vec![b'x'; 4096]).is_err());
        let mut s = Secret::empty();
        s.push("中a".as_bytes()).unwrap();
        s.pop_char();
        s.pop_char();
        assert!(s.is_empty());
    }
    #[test]
    fn idle_worker_clears_without_another_operation() {
        let session = Session::new(Duration::from_millis(25));
        {
            let mut s = session.shared.0.lock().unwrap();
            s.password = Some(Secret::from_bytes(b"synthetic".to_vec()));
            session.shared.1.notify_all();
        }
        thread::sleep(Duration::from_millis(100));
        assert!(session.shared.0.lock().unwrap().password.is_none());
    }
}

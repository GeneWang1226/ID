//! KeePassXC owns KDBX encryption; exported XML stays in wipeable memory only.
use crate::{Project, identity, private_dir, private_file, vault::Session};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use quick_xml::{
    Reader, Writer,
    events::{BytesEnd, BytesStart, BytesText, Event},
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

#[derive(Clone)]
pub(crate) struct Node {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub text: Zeroizing<String>,
    pub children: Vec<Node>,
}
impl Node {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.into(),
            attrs: Vec::new(),
            text: Zeroizing::new(String::new()),
            children: Vec::new(),
        }
    }
    pub fn leaf(name: &str, value: &str) -> Self {
        let mut node = Self::new(name);
        node.text.push_str(value);
        node
    }
    fn append_text(&mut self, value: &str) {
        if self.text.len() + value.len() > self.text.capacity() {
            let mut next = Zeroizing::new(String::with_capacity(
                (self.text.len() + value.len()).next_power_of_two(),
            ));
            next.push_str(&self.text);
            self.text = next;
        }
        self.text.push_str(value);
    }
    pub fn child(&self, name: &str) -> Option<&Self> {
        self.children.iter().find(|n| n.name == name)
    }
    pub fn child_mut(&mut self, name: &str) -> Option<&mut Self> {
        self.children.iter_mut().find(|n| n.name == name)
    }
    pub fn value(&self, name: &str) -> &str {
        self.child(name).map(|n| n.text.as_str()).unwrap_or("")
    }
    pub fn set(&mut self, name: &str, value: &str) {
        if let Some(child) = self.child_mut(name) {
            child.text = Zeroizing::new(value.into());
        } else {
            self.children.push(Self::leaf(name, value));
        }
    }
    pub fn field(&self, key: &str) -> Option<&Self> {
        self.children
            .iter()
            .find(|n| n.name == "String" && n.value("Key") == key)
            .and_then(|n| n.child("Value"))
    }
    pub fn field_value(&self, key: &str) -> &str {
        self.field(key).map(|n| n.text.as_str()).unwrap_or("")
    }
    pub fn set_field(&mut self, key: &str, value: &str, protected: bool) {
        self.children
            .retain(|n| n.name != "String" || n.value("Key") != key);
        let mut string = Self::new("String");
        string.children.push(Self::leaf("Key", key));
        let mut text = Self::leaf("Value", value);
        if protected {
            text.attrs.push(("ProtectInMemory".into(), "True".into()));
        }
        string.children.push(text);
        self.children.push(string);
    }
    pub fn remove_field(&mut self, key: &str) {
        self.children
            .retain(|n| n.name != "String" || n.value("Key") != key);
    }
    pub fn meta(&self, key: &str) -> &str {
        self.child("CustomData")
            .and_then(|d| {
                d.children
                    .iter()
                    .find(|n| n.name == "Item" && n.value("Key") == key)
            })
            .map(|n| n.value("Value"))
            .unwrap_or("")
    }
    pub fn set_meta(&mut self, key: &str, value: &str) {
        if self.child("CustomData").is_none() {
            self.children.push(Self::new("CustomData"));
        }
        let data = self.child_mut("CustomData").unwrap();
        data.children
            .retain(|n| n.name != "Item" || n.value("Key") != key);
        if !value.is_empty() {
            let mut item = Self::new("Item");
            item.children
                .extend([Self::leaf("Key", key), Self::leaf("Value", value)]);
            data.children.push(item);
        }
    }
    pub fn touch(&mut self) {
        let old = self.child("Times").and_then(|t| {
            chrono::DateTime::parse_from_rfc3339(t.value("LastModificationTime")).ok()
        });
        let mut time = chrono::Utc::now();
        if let Some(old) = old {
            time = time.max(old.with_timezone(&chrono::Utc) + chrono::Duration::seconds(1));
        }
        if self.child("Times").is_none() {
            self.children.push(Self::new("Times"));
        }
        self.child_mut("Times").unwrap().set(
            "LastModificationTime",
            &time.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        );
    }
    fn relocate(&mut self) -> Result<()> {
        let old = self
            .child("Times")
            .and_then(|t| chrono::DateTime::parse_from_rfc3339(t.value("LocationChanged")).ok());
        self.touch();
        let times = self.child_mut("Times").unwrap();
        let mut changed =
            chrono::DateTime::parse_from_rfc3339(times.value("LastModificationTime"))?
                .with_timezone(&chrono::Utc);
        if let Some(old) = old {
            changed = changed.max(old.with_timezone(&chrono::Utc) + chrono::Duration::seconds(1));
        }
        let changed = changed.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        times.set("LastModificationTime", &changed);
        times.set("LocationChanged", &changed);
        Ok(())
    }
}

pub(crate) fn protected(node: &Node) -> bool {
    node.attrs.iter().any(|(k, v)| {
        matches!(k.as_str(), "Protected" | "ProtectInMemory") && v.eq_ignore_ascii_case("true")
    })
}

fn new_group(name: &str, icon: u32) -> Node {
    let mut group = Node::new("Group");
    group.set("UUID", &STANDARD.encode(rand::random::<[u8; 16]>()));
    group.set("Name", name);
    group.set("IconID", &icon.to_string());
    group.set("IsExpanded", "True");
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let mut times = Node::new("Times");
    for key in [
        "CreationTime",
        "LastAccessTime",
        "LastModificationTime",
        "ExpiryTime",
        "LocationChanged",
    ] {
        times.set(key, &now);
    }
    times.set("Expires", "False");
    times.set("UsageCount", "0");
    group.children.push(times);
    group
}

#[cfg(target_os = "linux")]
fn xml_input(xml: &[u8]) -> Result<(fs::File, PathBuf)> {
    use std::os::fd::AsRawFd;
    let memory = rustix::fs::memfd_create("id-vault-xml", rustix::fs::MemfdFlags::CLOEXEC)?;
    let mut memory = fs::File::from(memory);
    memory.write_all(xml)?;
    let input = PathBuf::from(format!(
        "/proc/{}/fd/{}",
        std::process::id(),
        memory.as_raw_fd()
    ));
    Ok((memory, input))
}

pub(crate) fn create(path: &Path, name: &str, password: &crate::vault::Secret) -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (path, name, password);
        anyhow::bail!("当前平台尚不支持安全初始化，请在 Linux 创建密码库");
    }
    #[cfg(target_os = "linux")]
    {
        use crate::vault::run_with;
        use std::os::unix::fs::PermissionsExt;
        let mut doc = Document {
            root: parse(
                b"<KeePassFile><Meta><Generator>ID</Generator></Meta><Root/></KeePassFile>",
            )?,
            fingerprint: [0; 32],
            deletions: Vec::new(),
            original_paths: Default::default(),
        };
        let mut root = new_group(name, 48);
        let mut info = new_group("ID_info", 9);
        info.set_meta("ID.Group", "info");
        root.children.push(info);
        doc.root.child_mut("Root").unwrap().children.push(root);
        let id = doc.add(name, "info", 9)?;
        let entry = doc.entry_mut(&id)?;
        entry.set_field("身份名", name, false);
        entry.set_meta("ID.Category:身份名", "id-info");
        entry.touch();
        let xml = serialize(&doc.root)?;
        let (_memory, input) = xml_input(&xml)?;
        let temporary = tempfile::Builder::new()
            .prefix(".id-new-")
            .tempdir_in(path.parent().context("密码库目录不存在")?)?;
        fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700))?;
        let staged = temporary.path().join("new.kdbx");
        run_with(
            &input,
            "import",
            &[staged.to_string_lossy().into_owned(), "-p".into()],
            password,
            &[password],
        )?;
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o600))?;
        let actual = Document {
            root: parse(&run_with(&staged, "export", &[], password, &[])?)?,
            fingerprint: [0; 32],
            deletions: Vec::new(),
            original_paths: Default::default(),
        };
        let entries = actual.entries();
        ensure!(
            entries.len() == 1
                && entries[0].0 == format!("ID_info/{name}")
                && entries[0].1.value("UUID") == id
                && entries[0].1.meta("ID.Kind") == "info"
                && entries[0].1.field_value("身份名") == name
                && entries[0].1.field_value("Password").is_empty()
                && actual
                    .groups()
                    .iter()
                    .any(|(p, g)| p == "ID_info" && g.meta("ID.Group") == "info"),
            "新库初始化校验失败，未发布密码库"
        );
        fs::File::open(&staged)?.sync_all()?;
        // Publish the complete vault atomically, without replacing an existing identity's vault.
        fs::hard_link(&staged, path).context("密码库已存在或无法发布，未覆盖")?;
        Ok(())
    }
}

pub(crate) fn parse(xml: &[u8]) -> Result<Node> {
    fn inner(xml: &[u8]) -> Result<Node> {
        let mut reader = Reader::from_reader(xml);
        let mut stack = Vec::<Node>::new();
        let mut root = None;
        loop {
            let event = reader.read_event()?;
            match &event {
                Event::Start(e) | Event::Empty(e) => {
                    ensure!(stack.len() < 128, "XML nesting limit");
                    let mut node = Node::new(std::str::from_utf8(e.name().as_ref())?);
                    for attr in e.attributes() {
                        let attr = attr?;
                        node.attrs.push((
                            std::str::from_utf8(attr.key.as_ref())?.into(),
                            attr.decode_and_unescape_value(reader.decoder())?
                                .into_owned(),
                        ));
                    }
                    if matches!(event, Event::Empty(_)) {
                        if let Some(parent) = stack.last_mut() {
                            parent.children.push(node);
                        } else {
                            ensure!(root.is_none(), "Multiple roots");
                            root = Some(node);
                        }
                    } else {
                        stack.push(node);
                    }
                }
                Event::End(_) => {
                    let node = stack.pop().context("Invalid closing element")?;
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(node);
                    } else {
                        ensure!(root.is_none(), "Multiple roots");
                        root = Some(node);
                    }
                }
                Event::Text(_) | Event::CData(_) | Event::GeneralRef(_) => {
                    if let Some(node) = stack.last_mut() {
                        node.append_text(&crate::xml_text(&event)?.unwrap());
                    }
                }
                Event::DocType(_) => anyhow::bail!("DTD is not supported"),
                Event::Eof => break,
                _ => {}
            }
        }
        ensure!(stack.is_empty(), "Incomplete XML");
        let root = root.context("Missing root")?;
        ensure!(root.name == "KeePassFile", "Unexpected root");
        Ok(root)
    }
    inner(xml).map_err(|_| anyhow::anyhow!("KeePassXC 返回的资料结构无效，未修改密码库"))
}

pub(crate) fn serialize(root: &Node) -> Result<Zeroizing<Vec<u8>>> {
    fn write<W: Write>(node: &Node, out: &mut Writer<&mut W>) -> Result<()> {
        let mut start = BytesStart::new(&node.name);
        for (key, value) in &node.attrs {
            start.push_attribute((key.as_str(), value.as_str()));
        }
        out.write_event(Event::Start(start))?;
        let mut start = 0;
        for (index, c) in node.text.char_indices() {
            let escape = match c {
                '&' => "&amp;",
                '<' => "&lt;",
                '>' => "&gt;",
                _ => continue,
            };
            out.write_event(Event::Text(BytesText::from_escaped(
                &node.text[start..index],
            )))?;
            out.write_event(Event::Text(BytesText::from_escaped(escape)))?;
            start = index + c.len_utf8();
        }
        out.write_event(Event::Text(BytesText::from_escaped(&node.text[start..])))?;
        for child in &node.children {
            write(child, out)?;
        }
        out.write_event(Event::End(BytesEnd::new(&node.name)))?;
        Ok(())
    }
    struct Count(usize);
    impl Write for Count {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 += bytes.len();
            if self.0 > 64 * 1024 * 1024 {
                return Err(std::io::Error::other("密码库资料过大，请减少大型附件"));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    // Measure first so the plaintext buffer never reallocates.
    let mut count = Count(0);
    write(root, &mut Writer::new(&mut count))?;
    let mut bytes = Zeroizing::new(Vec::with_capacity(count.0));
    write(root, &mut Writer::new(&mut *bytes))?;
    Ok(bytes)
}

pub(crate) struct Document {
    pub root: Node,
    pub fingerprint: [u8; 32],
    pub deletions: Vec<String>,
    pub original_paths: std::collections::BTreeMap<String, String>,
}
fn fingerprint(path: &Path) -> Result<[u8; 32]> {
    let mut input = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(hash.finalize().into())
}
impl Document {
    fn take(&mut self, kind: &str, uuid: &str) -> Result<Node> {
        fn remove(group: &mut Node, kind: &str, uuid: &str) -> Option<Node> {
            if let Some(i) = group
                .children
                .iter()
                .position(|n| n.name == kind && n.value("UUID") == uuid)
            {
                return Some(group.children.remove(i));
            }
            group
                .children
                .iter_mut()
                .filter(|n| n.name == "Group")
                .find_map(|g| remove(g, kind, uuid))
        }
        let root = self
            .root
            .child_mut("Root")
            .and_then(|r| r.child_mut("Group"))
            .context("密码库缺少根分组")?;
        remove(root, kind, uuid).context("原分组或条目不存在")
    }
    pub fn organize(&mut self, identity: &str) -> Result<usize> {
        let info = crate::store::information_id(self)?;
        let groups = self.groups();
        let marked = groups
            .iter()
            .filter(|(_, g)| g.meta("ID.Group") == "info")
            .collect::<Vec<_>>();
        let targets = groups
            .iter()
            .filter(|(p, _)| p == "ID_info")
            .collect::<Vec<_>>();
        ensure!(
            marked.len() <= 1 && targets.len() <= 1,
            "基础信息群组标记或路径重复，原库保留"
        );
        let existing = marked
            .first()
            .or_else(|| targets.first())
            .map(|(p, g)| (p.to_owned(), g.value("UUID").to_owned()));
        if let (Some((_, g)), Some(target)) = (&existing, targets.first()) {
            ensure!(
                g == target.1.value("UUID"),
                "目标群组 ID_info 已存在，原库保留，请在 KeePassXC 核对"
            );
        }
        let mut changed = false;
        let group = if let Some((path, uuid)) = existing {
            self.group_mut(&uuid)?;
            if path != "ID_info" {
                let mut group = self.take("Group", &uuid)?;
                group.set("Name", "ID_info");
                group.relocate()?;
                self.root
                    .child_mut("Root")
                    .unwrap()
                    .child_mut("Group")
                    .unwrap()
                    .children
                    .push(group);
                changed = true;
            }
            let group = self.group_mut(&uuid)?;
            ensure!(
                group.meta("ID.Group").is_empty() || group.meta("ID.Group") == "info",
                "目标群组已用于其他资料，原库保留"
            );
            if group.meta("ID.Group") != "info" {
                ensure!(
                    group
                        .children
                        .iter()
                        .filter(|n| n.name == "Entry" || n.name == "Group")
                        .all(|e| e.name == "Entry" && info.as_deref() == Some(e.value("UUID"))),
                    "ID_info 含未关联资料，原库保留，请先在 KeePassXC 核对"
                );
                group.set_meta("ID.Group", "info");
                group.touch();
                changed = true;
            }
            uuid
        } else {
            let mut group = new_group("ID_info", 9);
            group.set_meta("ID.Group", "info");
            let uuid = group.value("UUID").to_owned();
            self.root
                .child_mut("Root")
                .and_then(|r| r.child_mut("Group"))
                .context("密码库缺少根分组")?
                .children
                .push(group);
            changed = true;
            uuid
        };
        let target = format!("ID_info/{identity}");
        let at_target = self.find_path(&target)?;
        ensure!(
            at_target.is_none() || at_target == info,
            "目标群组已有同名基础信息，原库保留"
        );
        let id = match info {
            Some(id) => id,
            None => {
                changed = true;
                self.add(identity, "info", 9)?
            }
        };
        let current = self
            .entries()
            .into_iter()
            .find(|(_, e)| e.value("UUID") == id)
            .context("基础信息不存在")?
            .0;
        if current != target {
            let mut entry = self.take("Entry", &id)?;
            let protected_title = entry.field("Title").is_some_and(protected);
            entry.set_field("Title", identity, protected_title);
            entry.relocate()?;
            self.group_mut(&group)?.children.push(entry);
            changed = true;
        }
        let entry = self.entry_mut(&id)?;
        if entry.meta("ID.Kind") != "info" {
            entry.set_meta("ID.Kind", "info");
            entry.touch();
            changed = true;
        }
        if entry.field("身份名").is_none() {
            entry.set_field("身份名", identity, false);
            entry.set_meta("ID.Category:身份名", "id-info");
            entry.touch();
            changed = true;
        }
        Ok(self.organize_root_entries()? + usize::from(changed))
    }
    pub fn groups(&self) -> Vec<(String, &Node)> {
        fn collect<'a>(
            group: &'a Node,
            path: String,
            recycle: &str,
            out: &mut Vec<(String, &'a Node)>,
        ) {
            if !recycle.is_empty() && group.value("UUID") == recycle {
                return;
            }
            out.push((path.clone(), group));
            for child in group.children.iter().filter(|n| n.name == "Group") {
                let next = if path.is_empty() {
                    child.value("Name").into()
                } else {
                    format!("{path}/{}", child.value("Name"))
                };
                collect(child, next, recycle, out);
            }
        }
        let recycle = self
            .root
            .child("Meta")
            .map(|m| m.value("RecycleBinUUID"))
            .unwrap_or("");
        let mut out = Vec::new();
        if let Some(root) = self.root.child("Root").and_then(|r| r.child("Group")) {
            collect(root, String::new(), recycle, &mut out);
        }
        out
    }
    fn group_mut(&mut self, uuid: &str) -> Result<&mut Node> {
        fn find<'a>(group: &'a mut Node, uuid: &str) -> Option<&'a mut Node> {
            if group.value("UUID") == uuid {
                return Some(group);
            }
            group
                .children
                .iter_mut()
                .filter(|n| n.name == "Group")
                .find_map(|n| find(n, uuid))
        }
        ensure!(
            !uuid.is_empty()
                && self
                    .groups()
                    .iter()
                    .filter(|(_, g)| g.value("UUID") == uuid)
                    .count()
                    == 1,
            "分组不存在或标识重复"
        );
        find(
            self.root
                .child_mut("Root")
                .and_then(|r| r.child_mut("Group"))
                .context("密码库缺少根分组")?,
            uuid,
        )
        .context("分组不存在")
    }
    fn managed_group(&mut self, role: &str) -> Result<String> {
        let found = self
            .groups()
            .into_iter()
            .filter(|(_, g)| g.meta("ID.Group") == role)
            .map(|(_, g)| g.value("UUID").to_owned())
            .collect::<Vec<_>>();
        ensure!(
            found.len() <= 1,
            "项目分组标记重复，请先在 KeePassXC 中整理"
        );
        if let Some(id) = found.first() {
            return Ok(id.clone());
        }
        let (name, icon) = match role {
            "root" => ("ID", 48),
            "info" => ("身份信息", 9),
            "account" => ("账号", 48),
            "event" => ("事件", 22),
            _ => anyhow::bail!("不支持的条目类型"),
        };
        let parent = if role == "root" {
            let root = self
                .root
                .child_mut("Root")
                .and_then(|r| r.child_mut("Group"))
                .context("密码库缺少根分组")?;
            if root.value("UUID").is_empty() {
                root.set("UUID", &STANDARD.encode(rand::random::<[u8; 16]>()));
            }
            root.value("UUID").to_owned()
        } else {
            self.managed_group("root")?
        };
        let parent = self.group_mut(&parent)?;
        let indices = parent
            .children
            .iter()
            .enumerate()
            .filter(|(_, g)| g.name == "Group" && g.value("Name") == name)
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        ensure!(indices.len() <= 1, "同一路径有重名分组，请先整理");
        let index = if let Some(i) = indices.first() {
            *i
        } else {
            parent.children.push(new_group(name, icon));
            parent.children.len() - 1
        };
        let group = &mut parent.children[index];
        ensure!(group.meta("ID.Group").is_empty(), "分组已用于其他项目类型");
        group.set_meta("ID.Group", role);
        group.touch();
        Ok(group.value("UUID").to_owned())
    }
    pub fn load(project: &Project, identity: &str, session: &Session) -> Result<Self> {
        let vault = identity::vault(project, identity)?;
        let before = fingerprint(&vault)?;
        let xml = session.run(&vault, "export", &[], &[])?;
        ensure!(
            before == fingerprint(&vault)?,
            "密码库正在被其他程序或同步更新，请刷新后重试"
        );
        let mut doc = Self {
            root: parse(&xml)?,
            fingerprint: before,
            deletions: Vec::new(),
            original_paths: Default::default(),
        };
        doc.original_paths = doc
            .entries()
            .into_iter()
            .map(|(path, e)| (e.value("UUID").to_owned(), path))
            .collect();
        Ok(doc)
    }
    pub fn entries(&self) -> Vec<(String, &Node)> {
        fn collect<'a>(
            group: &'a Node,
            parent: &str,
            recycle: &str,
            result: &mut Vec<(String, &'a Node)>,
        ) {
            if !recycle.is_empty() && group.value("UUID") == recycle {
                return;
            }
            for node in &group.children {
                if node.name == "Group" {
                    let path = if parent.is_empty() {
                        node.value("Name").into()
                    } else {
                        format!("{parent}/{}", node.value("Name"))
                    };
                    collect(node, &path, recycle, result);
                } else if node.name == "Entry" {
                    let title = node.field_value("Title");
                    result.push((
                        if parent.is_empty() {
                            title.into()
                        } else {
                            format!("{parent}/{title}")
                        },
                        node,
                    ));
                }
            }
        }
        let recycle = self
            .root
            .child("Meta")
            .map(|m| m.value("RecycleBinUUID"))
            .unwrap_or("");
        let mut result = Vec::new();
        if let Some(group) = self.root.child("Root").and_then(|r| r.child("Group")) {
            collect(group, "", recycle, &mut result);
        }
        result
    }
    pub fn entry_mut(&mut self, uuid: &str) -> Result<&mut Node> {
        fn find<'a>(node: &'a mut Node, uuid: &str) -> Option<&'a mut Node> {
            if node.name == "Entry" && node.value("UUID") == uuid {
                return Some(node);
            }
            if node.name == "History" {
                return None;
            }
            node.children.iter_mut().find_map(|n| find(n, uuid))
        }
        let matches = self
            .entries()
            .iter()
            .filter(|(_, n)| n.value("UUID") == uuid)
            .count();
        ensure!(matches == 1, "条目不存在或标识重复，请刷新后重试");
        find(&mut self.root, uuid).context("条目不存在")
    }
    pub fn add(&mut self, title: &str, kind: &str, icon: u32) -> Result<String> {
        let (group, title) =
            if let Some((parent, title)) = title.trim_start_matches('/').rsplit_once('/') {
                ensure!(kind != "info", "基础信息请使用项目专属分组");
                let found = self
                    .groups()
                    .into_iter()
                    .filter(|(path, _)| path == parent)
                    .map(|(_, g)| g.value("UUID").to_owned())
                    .collect::<Vec<_>>();
                ensure!(
                    found.len() == 1,
                    "目标分组不存在或重名，请先在 KeePassXC 中创建"
                );
                (found[0].clone(), title)
            } else {
                (self.managed_group(kind)?, title)
            };
        ensure!(!title.trim().is_empty(), "条目标题不能为空");
        ensure!(
            kind == "info" || title != "基础信息",
            "基础信息为保留条目，请换一个标题"
        );
        let uuid = STANDARD.encode(rand::random::<[u8; 16]>());
        let mut entry = Node::new("Entry");
        entry.set("UUID", &uuid);
        entry.set("IconID", &icon.to_string());
        entry.set_field("Title", title, false);
        entry.set_field("Password", "", true);
        entry.set_meta("ID.Kind", kind);
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let mut times = Node::new("Times");
        for key in [
            "CreationTime",
            "LastAccessTime",
            "LastModificationTime",
            "ExpiryTime",
            "LocationChanged",
        ] {
            times.set(key, &now);
        }
        times.set("Expires", "False");
        times.set("UsageCount", "0");
        entry.children.push(times);
        entry.touch();
        self.group_mut(&group)?.children.push(entry);
        Ok(uuid)
    }
    pub fn find_path(&self, path: &str) -> Result<Option<String>> {
        let found = self
            .entries()
            .into_iter()
            .filter(|(p, _)| p == path.trim_start_matches('/'))
            .map(|(_, n)| n.value("UUID").to_owned())
            .collect::<Vec<_>>();
        ensure!(
            found.len() <= 1,
            "同一路径有重名条目，请先在 KeePassXC 中整理"
        );
        Ok(found.into_iter().next())
    }
    pub fn find_entry(&self, name: &str) -> Result<Option<String>> {
        if let Some(id) = self.find_path(name)? {
            return Ok(Some(id));
        }
        let matches = self
            .entries()
            .into_iter()
            .filter(|(_, e)| e.field_value("Title").eq_ignore_ascii_case(name))
            .map(|(_, e)| e.value("UUID").to_owned())
            .collect::<Vec<_>>();
        ensure!(matches.len() <= 1, "存在同名条目，请提供完整分组路径");
        Ok(matches.into_iter().next())
    }
    pub fn root_project_entries(&self) -> Vec<(String, String)> {
        self.entries()
            .into_iter()
            .filter_map(|(path, e)| {
                if path.contains('/') {
                    return None;
                }
                let kind = e.meta("ID.Kind");
                let owned = matches!(kind, "info" | "account" | "event")
                    || e.meta("ID.PublicNotes") == "true"
                    || !e.meta("ID.Import").is_empty()
                    || e.child("CustomData").is_some_and(|d| {
                        d.children
                            .iter()
                            .any(|n| n.value("Key").starts_with("ID.LocalTransfer:"))
                    });
                owned.then(|| {
                    (
                        e.value("UUID").to_owned(),
                        if matches!(kind, "info" | "event") {
                            kind
                        } else {
                            "account"
                        }
                        .to_owned(),
                    )
                })
            })
            .collect()
    }
    pub fn organize_root_entries(&mut self) -> Result<usize> {
        let planned = self.root_project_entries();
        for (id, kind) in &planned {
            self.entry_mut(id)?;
            let group = self.managed_group(kind)?;
            let parent = self
                .groups()
                .into_iter()
                .find(|(_, g)| g.value("UUID") == group)
                .context("目标分组不存在")?
                .0;
            let title = self.entry_mut(id)?.field_value("Title").to_owned();
            ensure!(
                self.find_path(&format!("{parent}/{title}"))?.is_none(),
                "目标群组已有同名条目，原库保留，请先在 KeePassXC 中整理"
            );
            let mut entry = self.take("Entry", id)?;
            entry.relocate()?;
            self.group_mut(&group)?.children.push(entry);
        }
        Ok(planned.len())
    }
    #[cfg(target_os = "linux")]
    pub fn save(mut self, project: &Project, identity: &str, session: &Session) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let vault = identity::vault(project, identity)?;
        // KeePassXC 2.7 may omit KDBX 4 attachment bodies from XML exports.
        // Recover current attachments through its CLI; keep original histories in the merge target.
        let references = self
            .entries()
            .into_iter()
            .flat_map(|(path, e)| {
                e.children
                    .iter()
                    .filter(|n| n.name == "Binary")
                    .filter_map(move |b| {
                        b.child("Value")
                            .and_then(|v| v.attrs.iter().find(|(k, _)| k == "Ref"))
                            .map(|(_, id)| {
                                (
                                    path.clone(),
                                    e.value("UUID").to_owned(),
                                    b.value("Key").to_owned(),
                                    id.clone(),
                                )
                            })
                    })
            })
            .collect::<Vec<_>>();
        let mut attachment_checks = Vec::new();
        if let Some(meta) = self.root.child_mut("Meta") {
            meta.children.retain(|n| n.name != "Binaries");
        }
        let mut binary_ids = std::collections::HashSet::new();
        let mut size = 0;
        for (path, uuid, name, id) in references {
            ensure!(self.find_path(&path)?.is_some(), "附件条目路径有歧义");
            let source = self.original_paths.get(&uuid).unwrap_or(&path);
            ensure!(
                self.original_paths
                    .values()
                    .filter(|p| *p == source)
                    .count()
                    <= 1,
                "原附件路径有歧义，原库保留"
            );
            let bytes = session.run(
                &vault,
                "attachment-export",
                &["--stdout".into(), source.clone(), name.clone(), "-".into()],
                &[],
            )?;
            attachment_checks.push((path, name, Sha256::digest(&*bytes).into()));
            if !binary_ids.insert(id.clone()) {
                continue;
            }
            size += bytes.len();
            ensure!(
                size <= 48 * 1024 * 1024,
                "附件总量过大，请用 KeePassXC 管理"
            );
            let encoded = Zeroizing::new(STANDARD.encode(&*bytes));
            let mut binary = Node::leaf("Binary", &encoded);
            binary.attrs = vec![("ID".into(), id), ("Compressed".into(), "False".into())];
            let meta = self.root.child_mut("Meta").context("密码库缺少 Meta")?;
            if meta.child("Binaries").is_none() {
                meta.children.push(Node::new("Binaries"));
            }
            meta.child_mut("Binaries").unwrap().children.push(binary);
        }
        fn omit_history(n: &mut Node, recycle: &str, inside: bool) {
            let inside =
                inside || (!recycle.is_empty() && n.name == "Group" && n.value("UUID") == recycle);
            n.children
                .retain(|c| c.name != "History" && (!inside || c.name != "Entry"));
            for child in &mut n.children {
                omit_history(child, recycle, inside);
            }
        }
        let recycle = self
            .root
            .child("Meta")
            .map(|m| m.value("RecycleBinUUID"))
            .unwrap_or("")
            .to_owned();
        omit_history(&mut self.root, &recycle, false);
        let xml = serialize(&self.root)?;
        let (_memory, input) = xml_input(&xml)?;
        let temp = tempfile::Builder::new()
            .prefix(".id-transaction-")
            .tempdir_in(vault.parent().unwrap())?;
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700))?;
        let patch = temp.path().join("patch.kdbx");
        let staged = tempfile::NamedTempFile::new_in(temp.path())?.into_temp_path();
        fs::copy(&vault, &staged)?;
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o600))?;
        ensure!(
            fingerprint(&staged)? == self.fingerprint,
            "密码库已有新版本，请刷新后重试"
        );
        let verified = session.merge_xml(
            &vault,
            &input,
            &patch,
            &staged,
            &self.deletions,
            &attachment_checks,
        )?;
        let actual = Document {
            root: parse(&verified)?,
            fingerprint: [0; 32],
            deletions: Vec::new(),
            original_paths: Default::default(),
        };
        ensure!(
            actual.entries().len() == self.entries().len(),
            "保存校验失败：条目数量不一致，原库保留"
        );
        for (path, expected) in self.entries() {
            let matches = actual
                .entries()
                .into_iter()
                .filter(|(_, e)| e.value("UUID") == expected.value("UUID"))
                .collect::<Vec<_>>();
            ensure!(matches.len() == 1, "保存校验失败：条目未完整写入，原库保留");
            ensure!(
                matches[0].0 == path,
                "保存校验失败：条目分组不一致，原库保留"
            );
            let saved = matches[0].1;
            for field in expected.children.iter().filter(|n| n.name == "String") {
                let key = field.value("Key");
                ensure!(
                    saved.field_value(key) == expected.field_value(key),
                    "保存校验失败：字段不一致，原库保留"
                );
                ensure!(
                    expected.field(key).is_none_or(|n| !protected(n))
                        || saved.field(key).is_some_and(protected),
                    "保存校验失败：字段保护属性丢失，原库保留"
                );
            }
            for field in saved.children.iter().filter(|n| n.name == "String") {
                let key = field.value("Key");
                ensure!(
                    expected.field(key).is_some()
                        || (matches!(key, "Title" | "UserName" | "Password" | "URL" | "Notes")
                            && saved.field_value(key).is_empty()),
                    "保存校验失败：应删除的字段仍存在，原库保留"
                );
            }
            ensure!(
                saved.value("IconID") == expected.value("IconID")
                    || expected.value("IconID").is_empty(),
                "保存校验失败：图标不一致，原库保留"
            );
            ensure!(
                saved.value("CustomIconUUID") == expected.value("CustomIconUUID"),
                "保存校验失败：自定义图标不一致，原库保留"
            );
        }
        for (path, expected) in self
            .groups()
            .into_iter()
            .filter(|(_, g)| !g.meta("ID.Group").is_empty())
        {
            let matches = actual
                .groups()
                .into_iter()
                .filter(|(_, g)| g.value("UUID") == expected.value("UUID"))
                .collect::<Vec<_>>();
            ensure!(
                matches.len() == 1
                    && matches[0].0 == path
                    && matches[0].1.meta("ID.Group") == expected.meta("ID.Group"),
                "保存校验失败：项目分组未完整保留，原库保留"
            );
        }
        for path in &self.deletions {
            ensure!(actual.find_path(path)?.is_none(), "删除校验失败，原库保留");
        }
        ensure!(
            fingerprint(&vault)? == self.fingerprint,
            "密码库已被其他程序或同步修改，本次修改未覆盖，请刷新后重试"
        );
        backup(project, identity)?;
        fs::File::open(&staged)?.sync_all()?;
        ensure!(
            fingerprint(&vault)? == self.fingerprint,
            "密码库已被同步修改，请刷新后重试"
        );
        staged
            .persist(&vault)
            .map_err(|_| anyhow::anyhow!("保存失败，原库和加密备份已保留"))?;
        fs::File::open(vault.parent().unwrap())?.sync_all()?;
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    pub fn save(self, _project: &Project, _identity: &str, _session: &Session) -> Result<()> {
        anyhow::bail!(
            "此版本的资料编辑需要 Linux 匿名内存文件；其他平台可用 KeePassXC 编辑同一密码库"
        )
    }
}

pub(crate) struct WriteLock {
    _file: fs::File,
}
impl WriteLock {
    pub fn acquire(project: &Project, identity: &str) -> Result<Self> {
        identity::vault(project, identity)?;
        let folder = project.root.join("config/.cache/vault-locks");
        project.contained(&folder)?;
        private_dir(&folder)?;
        let path = folder.join(format!("{identity}.lock"));
        project.contained(&path)?;
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path)?;
        file.try_lock()
            .map_err(|_| anyhow::anyhow!("另一个 ID 操作正在修改此密码库，请稍后重试"))?;
        Ok(Self { _file: file })
    }
}

pub fn backup(project: &Project, identity: &str) -> Result<PathBuf> {
    let folder = project.root.join("backups/vaults").join(identity);
    project.contained(&folder)?;
    private_dir(&folder)?;
    let path = folder.join(format!(
        "{}.kdbx",
        chrono::Local::now().format("%Y%m%d-%H%M%S-%f")
    ));
    let mut file = private_file(&path)?;
    std::io::copy(
        &mut fs::File::open(identity::vault(project, identity)?)?,
        &mut file,
    )?;
    file.sync_all()?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn xml_roundtrip_preserves_unknown_data_and_protection() {
        let xml = br#"<KeePassFile><Meta><CustomIcons><Icon><UUID>icon</UUID><Data>YWJj</Data></Icon></CustomIcons></Meta><Root><Group><Name>Root</Name><Entry><UUID>entry</UUID><String><Key>Title</Key><Value>A &amp; B</Value></String><String><Key>Secret</Key><Value ProtectInMemory="True">synthetic</Value></String><History><Entry><UUID>entry</UUID><Tags>old</Tags></Entry></History><Unknown a="b">keep</Unknown></Entry></Group></Root></KeePassFile>"#;
        let mut doc = Document {
            root: parse(xml).unwrap(),
            fingerprint: [0; 32],
            deletions: Vec::new(),
            original_paths: Default::default(),
        };
        assert_eq!(doc.entries()[0].0, "A & B");
        let entry = doc.entry_mut("entry").unwrap();
        entry.set_field("Public", "<value>\n中文", false);
        entry.set_meta("ID.Kind", "account");
        let out = serialize(&doc.root).unwrap();
        let again = Document {
            root: parse(&out).unwrap(),
            fingerprint: [0; 32],
            deletions: Vec::new(),
            original_paths: Default::default(),
        };
        let entry = again.entries()[0].1;
        assert_eq!(entry.field_value("Public"), "<value>\n中文");
        assert!(protected(entry.field("Secret").unwrap()));
        assert_eq!(entry.meta("ID.Kind"), "account");
        assert!(entry.child("History").is_some());
        assert_eq!(entry.child("Unknown").unwrap().text.as_str(), "keep");
        assert!(
            again
                .root
                .child("Meta")
                .unwrap()
                .child("CustomIcons")
                .is_some()
        );
    }
    #[test]
    fn nested_groups_skip_recycle_bin_and_history() {
        let xml = b"<KeePassFile><Meta><RecycleBinUUID>bin</RecycleBinUUID></Meta><Root><Group><Name>Root</Name><Group><UUID>bin</UUID><Name>Bin</Name><Entry><String><Key>Title</Key><Value>Deleted</Value></String></Entry></Group><Group><Name>Accounts</Name><Entry><UUID>live</UUID><String><Key>Title</Key><Value>GitHub</Value></String><History><Entry><UUID>live</UUID></Entry></History></Entry></Group></Group></Root></KeePassFile>";
        let doc = Document {
            root: parse(xml).unwrap(),
            fingerprint: [0; 32],
            deletions: Vec::new(),
            original_paths: Default::default(),
        };
        assert_eq!(doc.entries().len(), 1);
        assert_eq!(doc.entries()[0].0, "Accounts/GitHub");
        assert!(parse(b"<!DOCTYPE x><KeePassFile/>").is_err());
    }
    #[test]
    fn native_project_groups_reuse_uuid_after_rename_and_keep_other_groups() {
        let mut d=Document{root:parse(b"<KeePassFile><Root><Group><UUID>root</UUID><Group><UUID>work</UUID><Name>Work</Name></Group></Group></Root></KeePassFile>").unwrap(),fingerprint:[0;32],deletions:Vec::new(),original_paths:Default::default()};
        d.add("基础信息", "info", 9).unwrap();
        d.add("GitHub", "account", 1).unwrap();
        d.add("注册", "event", 22).unwrap();
        assert_eq!(
            d.entries()
                .iter()
                .map(|(p, _)| p.as_str())
                .collect::<Vec<_>>(),
            ["ID/身份信息/基础信息", "ID/账号/GitHub", "ID/事件/注册"]
        );
        let id = d.managed_group("account").unwrap();
        d.group_mut(&id).unwrap().set("Name", "Accounts");
        d.add("Mail", "account", 19).unwrap();
        assert_eq!(d.managed_group("account").unwrap(), id);
        assert!(d.find_path("ID/Accounts/Mail").unwrap().is_some());
        let work = d.add("Work/Custom", "account", 1).unwrap();
        assert_eq!(
            d.find_path("Work/Custom").unwrap().as_deref(),
            Some(work.as_str())
        );
        assert!(d.add("Missing/Custom", "account", 1).is_err());
        let duplicate = d.group_mut(&id).unwrap().clone();
        d.root
            .child_mut("Root")
            .unwrap()
            .child_mut("Group")
            .unwrap()
            .children
            .push(duplicate);
        assert!(d.add("Unsafe", "account", 1).is_err());
    }
}

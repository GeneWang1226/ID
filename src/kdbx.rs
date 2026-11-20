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

const PROJECT_ROOT: &str = "ID";
const INFO_GROUP: &str = "ID_info";
const CONTACT_GROUP: &str = "ID_contacts";
const ACCOUNT_GROUP: &str = "ID_account";
const EMAIL_GROUP: &str = "ID_emails";
const EVENT_GROUP: &str = "ID_events";

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

pub(crate) fn create(
    path: &Path,
    name: &str,
    password: &crate::vault::Secret,
    language: &str,
) -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (path, name, password, language);
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
            original_root_uuid: None,
            original_root_name: None,
        };
        let mut root = new_group(PROJECT_ROOT, 48);
        root.set_meta("ID.Group", "root");
        let mut info = new_group(INFO_GROUP, 9);
        info.set_meta("ID.Group", "info");
        let mut contacts = new_group(CONTACT_GROUP, 5);
        contacts.set_meta("ID.Group", "contact");
        let mut accounts = new_group(ACCOUNT_GROUP, 48);
        accounts.set_meta("ID.Group", "account");
        let mut emails = new_group(EMAIL_GROUP, 19);
        emails.set_meta("ID.Group", "email");
        let mut events = new_group(EVENT_GROUP, 22);
        events.set_meta("ID.Group", "event");
        root.children.push(info);
        root.children.extend([contacts, accounts, emails, events]);
        doc.root.child_mut("Root").unwrap().children.push(root);
        let english = language == "en";
        let profile_title = if english {
            "Identity profile"
        } else {
            "身份档案"
        };
        let id = doc.add(profile_title, "info", 9)?;
        let entry = doc.entry_mut(&id)?;
        entry.set_meta("ID.InfoType", "profile");
        let profile_fields = if english {
            [
                "Identity name",
                "Name",
                "English name",
                "Nickname",
                "Birthday",
                "Nationality",
                "Document type",
                "Document number",
                "City",
                "Time zone",
                "Personal notes",
            ]
        } else {
            [
                "身份名",
                "姓名",
                "英文名",
                "昵称",
                "生日",
                "国籍",
                "证件类型",
                "证件号码",
                "常住城市",
                "时区",
                "个人备注",
            ]
        };
        entry.set_meta("ID.IdentityNameField", profile_fields[0]);
        for (index, key) in profile_fields.into_iter().enumerate() {
            let value = if index == 0 { name } else { "" };
            let protected = index == 7;
            entry.set_field(key, value, protected);
            entry.set_meta(&format!("ID.Category:{key}"), "id-info");
        }
        entry.touch();
        let dimensions = if english {
            [
                (
                    "Education",
                    "education",
                    [
                        "School",
                        "Degree",
                        "Major",
                        "Start date",
                        "End date",
                        "Location",
                        "Notes",
                    ],
                ),
                (
                    "Work history",
                    "work",
                    [
                        "Company",
                        "Role",
                        "Department",
                        "Start date",
                        "End date",
                        "Location",
                        "Notes",
                    ],
                ),
                (
                    "Address",
                    "address",
                    [
                        "Type",
                        "Country/region",
                        "State/province",
                        "City",
                        "Address",
                        "Postal code",
                        "Notes",
                    ],
                ),
                (
                    "Vehicle",
                    "vehicle",
                    [
                        "Make",
                        "Model",
                        "License plate",
                        "VIN",
                        "Purchase date",
                        "Insurance expiry",
                        "Notes",
                    ],
                ),
                (
                    "Health",
                    "health",
                    [
                        "Blood type",
                        "Allergies",
                        "Emergency contact",
                        "Notes",
                        "Medical notes",
                        "Insurance",
                        "Medication",
                    ],
                ),
            ]
        } else {
            [
                (
                    "教育经历",
                    "education",
                    [
                        "学校",
                        "学历",
                        "专业",
                        "开始时间",
                        "结束时间",
                        "所在地",
                        "备注",
                    ],
                ),
                (
                    "工作经历",
                    "work",
                    [
                        "公司",
                        "职位",
                        "部门",
                        "开始时间",
                        "结束时间",
                        "所在地",
                        "备注",
                    ],
                ),
                (
                    "地址信息",
                    "address",
                    [
                        "地址类型",
                        "国家/地区",
                        "省/州",
                        "城市",
                        "详细地址",
                        "邮编",
                        "备注",
                    ],
                ),
                (
                    "车辆信息",
                    "vehicle",
                    [
                        "品牌",
                        "型号",
                        "车牌号",
                        "VIN",
                        "购买日期",
                        "保险到期",
                        "备注",
                    ],
                ),
                (
                    "健康信息",
                    "health",
                    [
                        "血型",
                        "过敏史",
                        "紧急联系人",
                        "备注",
                        "医疗备注",
                        "保险信息",
                        "用药说明",
                    ],
                ),
            ]
        };
        for (title, kind, fields) in dimensions {
            let entry_id = doc.add(title, "info", 9)?;
            let entry = doc.entry_mut(&entry_id)?;
            entry.set_meta("ID.InfoType", kind);
            for key in fields {
                entry.set_field(
                    key,
                    "",
                    matches!(
                        key,
                        "VIN"
                            | "车牌号"
                            | "License plate"
                            | "过敏史"
                            | "Allergies"
                            | "保险信息"
                            | "Insurance"
                            | "用药说明"
                            | "Medication"
                    ),
                );
                entry.set_meta(&format!("ID.Category:{key}"), kind);
            }
            entry.touch();
        }
        let contacts = if english {
            [
                "Phone number · primary",
                "Phone number · work",
                "Phone number · backup",
            ]
        } else {
            ["手机号码 · 主", "手机号码 · 工作", "手机号码 · 备用"]
        };
        for title in contacts {
            let contact_id = doc.add(title, "contact", 5)?;
            doc.entry_mut(&contact_id)?
                .set_meta("ID.ContactType", "phone");
        }
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
            original_root_uuid: None,
            original_root_name: None,
        };
        let entries = actual.entries();
        ensure!(
            entries.iter().any(|(path, entry)| {
                path == &format!("{PROJECT_ROOT}/{INFO_GROUP}/{profile_title}")
                    && entry.value("UUID") == id
                    && entry.meta("ID.InfoType") == "profile"
                    && entry.field_value(profile_fields[0]) == name
                    && entry.field(profile_fields[7]).is_some_and(protected)
            }) && entries.iter().any(|(_, e)| e.meta("ID.Kind") == "info")
                && entries
                    .iter()
                    .filter(|(_, e)| e.meta("ID.Kind") == "info")
                    .count()
                    == 6
                && actual.groups().iter().any(|(p, g)| {
                    p == &format!("{PROJECT_ROOT}/{INFO_GROUP}") && g.meta("ID.Group") == "info"
                })
                && actual
                    .groups()
                    .iter()
                    .any(|(p, g)| p == PROJECT_ROOT && g.meta("ID.Group") == "root")
                && actual.groups().iter().any(|(p, g)| {
                    p == &format!("{PROJECT_ROOT}/{CONTACT_GROUP}")
                        && g.meta("ID.Group") == "contact"
                }),
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
    pub original_root_uuid: Option<String>,
    pub original_root_name: Option<String>,
}
pub(crate) fn fingerprint(path: &Path) -> Result<[u8; 32]> {
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

    fn root_container_mut(&mut self) -> Result<&mut Node> {
        self.root
            .child_mut("Root")
            .and_then(|root| root.child_mut("Group"))
            .context("密码库缺少根分组")
    }

    fn promote_root(&mut self, uuid: &str) -> Result<()> {
        fn remove(group: &mut Node, uuid: &str) -> Option<Node> {
            if let Some(index) = group
                .children
                .iter()
                .position(|node| node.name == "Group" && node.value("UUID") == uuid)
            {
                return Some(group.children.remove(index));
            }
            for child in group.children.iter_mut().filter(|n| n.name == "Group") {
                if let Some(found) = remove(child, uuid) {
                    return Some(found);
                }
            }
            None
        }
        let root = self.root.child_mut("Root").context("密码库缺少根容器")?;
        let container = root
            .children
            .iter_mut()
            .find(|n| n.name == "Group")
            .context("密码库缺少根分组")?;
        let mut project = remove(container, uuid).context("ID 根分组不存在")?;
        for child in std::mem::take(&mut container.children) {
            if matches!(child.name.as_str(), "Group" | "Entry") {
                project.children.push(child);
            }
        }
        project.set("Name", PROJECT_ROOT);
        project.set_meta("ID.Group", "root");
        project.relocate()?;
        root.children.clear();
        root.children.push(project);
        Ok(())
    }

    fn group_selection(&self, role: &str, names: &[&str]) -> Result<Option<(String, String)>> {
        let marked = self
            .groups()
            .into_iter()
            .filter(|(_, group)| group.meta("ID.Group") == role)
            .map(|(path, group)| (path, group.value("UUID").to_owned()))
            .collect::<Vec<_>>();
        ensure!(
            marked.len() <= 1,
            "项目分组标记重复，请先在 KeePassXC 中整理"
        );
        let named = self
            .groups()
            .into_iter()
            .filter(|(path, _)| names.iter().any(|name| path == name))
            .map(|(path, group)| (path, group.value("UUID").to_owned()))
            .collect::<Vec<_>>();
        ensure!(
            named.len() <= 1,
            "项目分组路径重复，请先在 KeePassXC 中整理"
        );
        if let (Some(marked), Some(named)) = (marked.first(), named.first()) {
            ensure!(marked.1 == named.1, "项目分组标记与路径冲突，原库保留");
        }
        Ok(marked.first().cloned().or_else(|| named.first().cloned()))
    }

    fn canonical_root(&mut self) -> Result<(String, bool)> {
        let selected = self.group_selection("root", &[PROJECT_ROOT, "Passwords/ID"])?;
        let actual_uuid = self.root_container_mut()?.value("UUID").to_owned();
        if self.root_container_mut()?.value("Name") == PROJECT_ROOT {
            let root = self.root_container_mut()?;
            let changed = root.meta("ID.Group") != "root";
            if changed {
                root.set_meta("ID.Group", "root");
                root.touch();
            }
            return Ok((actual_uuid, changed));
        }
        let (uuid, mut changed) = if let Some((path, uuid)) = selected {
            ensure!(
                self.groups()
                    .into_iter()
                    .find(|(_, group)| group.value("UUID") == uuid)
                    .is_some_and(|(_, group)| group.meta("ID.Group").is_empty()
                        || group.meta("ID.Group") == "root"),
                "ID 目标群组已用于其他资料，原库保留"
            );
            let is_actual_root = uuid == actual_uuid;
            if path != PROJECT_ROOT || !is_actual_root {
                self.promote_root(&uuid)?;
            }
            (uuid, path != PROJECT_ROOT || !is_actual_root)
        } else {
            let root = self.root_container_mut()?;
            if root.value("UUID").is_empty() {
                root.set("UUID", &STANDARD.encode(rand::random::<[u8; 16]>()));
            }
            root.set("Name", PROJECT_ROOT);
            root.set("IconID", "48");
            root.set_meta("ID.Group", "root");
            root.touch();
            let uuid = root.value("UUID").to_owned();
            (uuid, true)
        };
        let at_root = self
            .groups()
            .into_iter()
            .find(|(_, group)| group.value("UUID") == uuid)
            .is_some_and(|(path, _)| path == PROJECT_ROOT);
        if changed && !at_root {
            let mut root = self.take("Group", &uuid)?;
            root.set("Name", PROJECT_ROOT);
            root.set_meta("ID.Group", "root");
            root.relocate()?;
            self.root_container_mut()?.children.push(root);
        } else {
            let root = self.group_mut(&uuid)?;
            if root.meta("ID.Group") != "root" {
                root.set_meta("ID.Group", "root");
                root.touch();
                changed = true;
            }
            if root.value("Name") != PROJECT_ROOT {
                root.set("Name", PROJECT_ROOT);
                root.relocate()?;
                changed = true;
            }
        }
        Ok((uuid, changed))
    }

    fn canonical_child(
        &mut self,
        parent: &str,
        role: &str,
        name: &str,
        legacy: &[&str],
        icon: u32,
    ) -> Result<(String, bool)> {
        let target = format!("{PROJECT_ROOT}/{name}");
        let mut candidates = vec![target.as_str()];
        candidates.extend_from_slice(legacy);
        let selected = self.group_selection(role, &candidates)?;
        let (uuid, mut changed) = if let Some((path, uuid)) = selected {
            let group = self
                .groups()
                .into_iter()
                .find(|(_, group)| group.value("UUID") == uuid)
                .map(|(_, group)| group)
                .context("项目分组不存在")?;
            ensure!(
                group.meta("ID.Group").is_empty() || group.meta("ID.Group") == role,
                "目标群组已用于其他资料，原库保留"
            );
            (uuid, path != target)
        } else {
            let mut group = new_group(name, icon);
            group.set_meta("ID.Group", role);
            let uuid = group.value("UUID").to_owned();
            self.group_mut(parent)?.children.push(group);
            (uuid, true)
        };
        if changed {
            if let Some((_, other)) = self.groups().into_iter().find(|(path, _)| path == &target) {
                ensure!(other.value("UUID") == uuid, "目标群组路径冲突，原库保留");
            }
            let mut group = self.take("Group", &uuid)?;
            group.set("Name", name);
            group.set_meta("ID.Group", role);
            group.relocate()?;
            self.group_mut(parent)?.children.push(group);
        } else {
            let group = self.group_mut(&uuid)?;
            if group.meta("ID.Group") != role {
                group.set_meta("ID.Group", role);
                group.touch();
                changed = true;
            }
        }
        Ok((uuid, changed))
    }

    pub fn organize(&mut self, identity: &str) -> Result<usize> {
        let info = crate::store::information_id(self)?;
        let (root, root_changed) = self.canonical_root()?;
        let (info_group, info_changed) = self.canonical_child(
            &root,
            "info",
            INFO_GROUP,
            &[
                "ID_info",
                "Passwords/ID_info",
                "ID/身份信息",
                "ID/身份信息/基础信息",
                "Passwords/ID/身份信息",
            ],
            9,
        )?;
        let (_, account_changed) = self.canonical_child(
            &root,
            "account",
            ACCOUNT_GROUP,
            &["ID/账号", "ID/Accounts", "Passwords/ID/账号"],
            48,
        )?;
        let (_, contact_changed) = self.canonical_child(
            &root,
            "contact",
            CONTACT_GROUP,
            &[
                "ID/联系人",
                "Passwords/ID/联系人",
                "ID_contacts",
                "Passwords/ID_contacts",
            ],
            5,
        )?;
        let (_, event_changed) = self.canonical_child(
            &root,
            "event",
            EVENT_GROUP,
            &["ID/事件", "ID/Events", "Passwords/ID/事件"],
            22,
        )?;
        let (_, email_changed) = self.canonical_child(
            &root,
            "email",
            EMAIL_GROUP,
            &[
                "ID_emails",
                "Passwords/ID_emails",
                "ID/邮箱",
                "Passwords/ID/邮箱",
            ],
            19,
        )?;
        let mut changed = root_changed
            || info_changed
            || account_changed
            || contact_changed
            || event_changed
            || email_changed;
        let info_title = info.as_ref().and_then(|id| {
            self.entries()
                .into_iter()
                .find(|(_, entry)| entry.value("UUID") == id)
                .map(|(_, entry)| entry.field_value("Title").to_owned())
        });
        let target = info_title
            .filter(|title| !title.is_empty() && title != identity && title != "基础信息")
            .map(|title| format!("{PROJECT_ROOT}/{INFO_GROUP}/{title}"))
            .unwrap_or_else(|| format!("{PROJECT_ROOT}/{INFO_GROUP}/身份档案"));
        let at_target = self.find_path(&target)?;
        ensure!(
            at_target.is_none() || at_target == info,
            "目标群组已有同名基础信息，原库保留"
        );
        let id = match info {
            Some(id) => id,
            None => {
                changed = true;
                self.add("身份档案", "info", 9)?
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
            entry.set_field("Title", "身份档案", protected_title);
            entry.relocate()?;
            self.group_mut(&info_group)?.children.push(entry);
            changed = true;
        }
        let entry = self.entry_mut(&id)?;
        if entry.meta("ID.Kind") != "info" {
            entry.set_meta("ID.Kind", "info");
            entry.touch();
            changed = true;
        }
        if entry.meta("ID.InfoType").is_empty() {
            entry.set_meta("ID.InfoType", "profile");
            entry.touch();
            changed = true;
        }
        let identity_field = if entry.meta("ID.IdentityNameField").is_empty() {
            "身份名".to_owned()
        } else {
            entry.meta("ID.IdentityNameField").to_owned()
        };
        if entry.field(&identity_field).is_none() {
            entry.set_field(&identity_field, identity, false);
            entry.set_meta(&format!("ID.Category:{identity_field}"), "id-info");
            entry.set_meta("ID.IdentityNameField", &identity_field);
            entry.touch();
            changed = true;
        }
        let mut count = self.organize_root_entries()? + usize::from(changed);
        let email_groups = self
            .groups()
            .into_iter()
            .filter(|(path, group)| {
                group.meta("ID.Group") == "email"
                    || path == EMAIL_GROUP
                    || path == &format!("{PROJECT_ROOT}/{EMAIL_GROUP}")
            })
            .map(|(path, _)| path)
            .collect::<Vec<_>>();
        let emails = self
            .entries()
            .into_iter()
            .filter(|(path, e)| {
                let imported = !e.meta("ID.Import").is_empty()
                    || e.child("CustomData").is_some_and(|d| {
                        d.children
                            .iter()
                            .any(|n| n.value("Key").starts_with("ID.EmailTransfer:"))
                    });
                let provider = e.meta("ID.Kind") == "account"
                    && matches!(
                        e.field_value("服务商").to_ascii_lowercase().as_str(),
                        "163"
                            | "126"
                            | "qq"
                            | "outlook"
                            | "hotmail"
                            | "gmail"
                            | "proton"
                            | "icloud"
                            | "yahoo"
                    );
                !matches!(e.meta("ID.Kind"), "info" | "event")
                    && (e.meta("ID.AccountType") == "email"
                        || email_groups
                            .iter()
                            .any(|group| path.starts_with(&format!("{group}/")))
                        || (crate::store::email_address(e.field_value("UserName"))
                            && (imported || provider)))
            })
            .map(|(_, e)| e.value("UUID").to_owned())
            .collect::<Vec<_>>();
        for id in emails {
            count += usize::from(self.email(&id)?);
        }
        Ok(count)
    }
    pub fn email(&mut self, id: &str) -> Result<bool> {
        let entry = self.entry_mut(id)?;
        ensure!(
            !matches!(entry.meta("ID.Kind"), "info" | "event"),
            "基础信息或事件不能改为邮箱"
        );
        ensure!(
            crate::store::email_address(entry.field_value("UserName")),
            "请先填写有效邮箱地址"
        );
        let mut changed =
            entry.meta("ID.AccountType") != "email" || entry.meta("ID.Kind") != "account";
        entry.set_meta("ID.AccountType", "email");
        entry.set_meta("ID.Kind", "account");
        changed |= crate::store::promote_email_fields(entry);
        let marked_group = self
            .groups()
            .iter()
            .any(|(_, g)| g.meta("ID.Group") == "email");
        let group = self.managed_group("email")?;
        changed |= !marked_group;
        let parent = self
            .groups()
            .into_iter()
            .find(|(_, g)| g.value("UUID") == group)
            .context("邮箱群组不存在")?
            .0;
        let (current, title) = self
            .entries()
            .into_iter()
            .find(|(_, e)| e.value("UUID") == id)
            .map(|(path, e)| (path, e.field_value("Title").to_owned()))
            .context("邮箱条目不存在")?;
        // Keep user subgroups in the email group; explicit conversion only moves other entries.
        if !current.starts_with(&format!("{parent}/")) {
            ensure!(
                self.find_path(&format!("{parent}/{title}"))?.is_none(),
                "目标邮箱群组已有同名条目，原库保留"
            );
            let mut entry = self.take("Entry", id)?;
            entry.relocate()?;
            self.group_mut(&group)?.children.push(entry);
            changed = true;
        } else if changed {
            self.entry_mut(id)?.touch();
        }
        Ok(changed)
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
            let base = if root.value("Name") == PROJECT_ROOT {
                PROJECT_ROOT.to_owned()
            } else {
                String::new()
            };
            collect(root, base, recycle, &mut out);
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
            ensure!(
                role != "email"
                    || self
                        .groups()
                        .iter()
                        .all(|(path, g)| { path != EMAIL_GROUP || g.value("UUID") == id }),
                "目标邮箱群组已存在，请先核对"
            );
            return Ok(id.clone());
        }
        let (name, icon) = match role {
            "root" => (PROJECT_ROOT, 48),
            "info" => (INFO_GROUP, 9),
            "contact" => (CONTACT_GROUP, 5),
            "account" => (ACCOUNT_GROUP, 48),
            "event" => (EVENT_GROUP, 22),
            "email" => (EMAIL_GROUP, 19),
            _ => anyhow::bail!("不支持的条目类型"),
        };
        let parent = if role == "root" {
            let root = self.root_container_mut()?;
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
        ensure!(
            !matches!(role, "email" | "contact")
                || !group
                    .children
                    .iter()
                    .any(|e| e.name == "Entry" && matches!(e.meta("ID.Kind"), "info" | "event")),
            "邮箱群组含基础信息或事件条目，请先核对"
        );
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
            original_root_uuid: None,
            original_root_name: None,
        };
        doc.original_root_uuid = doc
            .root
            .child("Root")
            .and_then(|root| root.child("Group"))
            .map(|group| group.value("UUID").to_owned());
        doc.original_root_name = doc
            .root
            .child("Root")
            .and_then(|root| root.child("Group"))
            .map(|group| group.value("Name").to_owned());
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
            let base = if group.value("Name") == PROJECT_ROOT {
                PROJECT_ROOT.to_owned()
            } else {
                String::new()
            };
            collect(group, &base, recycle, &mut result);
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
        entry.set_meta("ID.Kind", if kind == "email" { "account" } else { kind });
        if kind == "email" {
            entry.set_meta("ID.AccountType", "email");
        }
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
                let path = path
                    .strip_prefix(&format!("{PROJECT_ROOT}/"))
                    .unwrap_or(&path);
                if path.contains('/') {
                    return None;
                }
                let kind = e.meta("ID.Kind");
                let owned = matches!(kind, "info" | "account" | "contact" | "event")
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
                        if matches!(kind, "info" | "contact" | "event") {
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
            let mut bytes = None;
            let parts = source.split('/').collect::<Vec<_>>();
            for start in 0..parts.len() {
                let candidate = parts[start..].join("/");
                if let Ok(value) = session.run(
                    &vault,
                    "attachment-export",
                    &["--stdout".into(), candidate, name.clone(), "-".into()],
                    &[],
                ) {
                    bytes = Some(value);
                    break;
                }
            }
            let bytes = bytes.context("附件条目路径不存在")?;
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
        let root_replaced = self
            .original_root_uuid
            .as_deref()
            .filter(|uuid| !uuid.is_empty())
            .is_some_and(|uuid| {
                self.root
                    .child("Root")
                    .and_then(|root| root.child("Group"))
                    .is_some_and(|group| group.value("UUID") != uuid)
            })
            || self.original_root_name.as_deref().is_some_and(|name| {
                self.root
                    .child("Root")
                    .and_then(|root| root.child("Group"))
                    .is_some_and(|group| group.value("Name") != name)
            });
        if !root_replaced {
            omit_history(&mut self.root, &recycle, false);
        }
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
        let verified = if root_replaced {
            let rebuilt = temp.path().join("rebuilt.kdbx");
            let verified = session.rebuild_xml(&vault, &input, &rebuilt, &attachment_checks)?;
            fs::copy(&rebuilt, &staged)?;
            verified
        } else {
            session.merge_xml(
                &vault,
                &input,
                &patch,
                &staged,
                &self.deletions,
                &attachment_checks,
            )?
        };
        let actual = Document {
            root: parse(&verified)?,
            fingerprint: [0; 32],
            deletions: Vec::new(),
            original_paths: Default::default(),
            original_root_uuid: None,
            original_root_name: None,
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
        crate::doctor::ensure_no_conflicts(&vault)?;
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
        let lock = Self::for_repair(project, identity)?;
        crate::doctor::ensure_no_conflicts(&identity::vault(project, identity)?)?;
        Ok(lock)
    }
    pub(crate) fn for_repair(project: &Project, identity: &str) -> Result<Self> {
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
            original_root_uuid: None,
            original_root_name: None,
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
            original_root_uuid: None,
            original_root_name: None,
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
            original_root_uuid: None,
            original_root_name: None,
        };
        assert_eq!(doc.entries().len(), 1);
        assert_eq!(doc.entries()[0].0, "Accounts/GitHub");
        assert!(parse(b"<!DOCTYPE x><KeePassFile/>").is_err());
    }
    #[test]
    fn native_project_groups_reuse_uuid_after_rename_and_keep_other_groups() {
        let mut d=Document{root:parse(b"<KeePassFile><Root><Group><UUID>root</UUID><Group><UUID>work</UUID><Name>Work</Name></Group></Group></Root></KeePassFile>").unwrap(),fingerprint:[0;32],deletions:Vec::new(),original_paths:Default::default(),original_root_uuid:None,original_root_name:None};
        d.add("基础信息", "info", 9).unwrap();
        d.add("GitHub", "account", 1).unwrap();
        d.add("注册", "event", 22).unwrap();
        assert_eq!(
            d.entries()
                .iter()
                .map(|(p, _)| p.as_str())
                .collect::<Vec<_>>(),
            [
                "ID/ID_info/基础信息",
                "ID/ID_account/GitHub",
                "ID/ID_events/注册",
            ]
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

    #[test]
    fn organize_moves_nested_project_root_to_first_level_and_keeps_passwords() {
        let xml = br#"
            <KeePassFile><Root><Group><UUID>container</UUID><Name>Root</Name>
              <Group><UUID>passwords</UUID><Name>Passwords</Name>
                <Group><UUID>project</UUID><Name>ID</Name>
                  <CustomData><Item><Key>ID.Group</Key><Value>root</Value></Item></CustomData>
                  <Group><UUID>info</UUID><Name>ID_info</Name>
                    <CustomData><Item><Key>ID.Group</Key><Value>info</Value></Item></CustomData>
                    <Entry><UUID>identity</UUID>
                      <String><Key>Title</Key><Value>alice</Value></String>
                      <String><Key>Password</Key><Value ProtectInMemory="True"/></String>
                      <CustomData><Item><Key>ID.Kind</Key><Value>info</Value></Item></CustomData>
                    </Entry>
                  </Group>
                </Group>
              </Group>
              <Group><UUID>keep</UUID><Name>Personal</Name></Group>
            </Group></Root></KeePassFile>
        "#;
        let mut doc = Document {
            root: parse(xml).unwrap(),
            fingerprint: [0; 32],
            deletions: Vec::new(),
            original_paths: Default::default(),
            original_root_uuid: None,
            original_root_name: None,
        };
        let count = doc.organize("alice").unwrap();
        assert!(count > 0);
        assert_eq!(
            doc.root
                .child("Root")
                .and_then(|root| root.child("Group"))
                .map(|group| group.value("Name")),
            Some("ID")
        );
        let project = doc
            .groups()
            .into_iter()
            .find(|(path, group)| path == PROJECT_ROOT && group.value("UUID") == "project");
        assert!(project.is_some());
        assert!(
            doc.groups()
                .iter()
                .any(|(path, group)| path == "ID/Passwords" && group.value("UUID") == "passwords")
        );
        assert!(
            doc.groups()
                .iter()
                .any(|(path, group)| path == "ID/Personal" && group.value("UUID") == "keep")
        );
        assert!(doc.find_path("ID/ID_info/身份档案").unwrap().is_some());
        assert!(doc.groups().iter().any(|(path, _)| path == "ID/ID_account"));
        fn has_removed(node: &Node) -> bool {
            node.name == "Removed" || node.children.iter().any(has_removed)
        }
        assert!(!has_removed(&doc.root));
    }
}

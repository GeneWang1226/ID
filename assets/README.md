# assets — 工具与安装包

存放跨身份通用的二进制文件，主要是各平台的密码工具安装包，方便在新设备上快速就绪。

## 建议放什么

| 文件 | 用途 | 下载地址 |
| --- | --- | --- |
| `KeePassXC-*-x64.msi` | Windows 密码库客户端（2.8.0 起官方改用 MSI，不再有 exe） | https://keepassxc.info/download/ |
| `KeePassXC-*-x86_64.AppImage` | Linux 免安装版 | 同上 |
| `KeePassDX-*.apk` | Android 客户端（支持 kdbx） | https://www.keepassdx.com/ |

## 注意

- **只放安装包，不放任何身份数据**——这里的东西丢了多少份都无所谓。
- 下载后核对官网公布的 SHA-256 校验值再收进来：
  ```bash
  sha256sum KeePassXC-*-Win64.exe
  ```
- 安装包体积较大，如果不想让它走 Syncthing 同步到每台设备，在 `.stignore` 里排除 `assets/`。
- 旧版本及时清理，只留最新一两个版本。

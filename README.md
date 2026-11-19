# ID

ID is a local-first identity manager for people who manage multiple identities. Its Rust TUI keeps each identity in an independent KeePassXC KDBX vault, with accounts, profiles, credentials, and events organized per identity and synchronized safely across devices.

## Highlights

- One isolated encrypted KDBX vault per identity.
- A single terminal workflow for switching between personal, work, project, and other identities.
- Native KeePass groups and entries for accounts, email, profile data, events, custom fields, and attachments.
- Full-screen terminal UI with local password and protected-field operations.
- Optional local AI assistance that receives metadata only; passwords and protected values stay local.
- Safe migration from older ID layouts with backups and conflict checks.
- No runtime database or web service.

## Quick start

Requirements: Linux, Rust 1.89+, GNU Make, and `keepassxc-cli`.

```sh
make build
make settings
make new id=example
make manage id=example
```

`make new` asks for an independent master password and creates `ID/ID_info/example` in `identities/example.kdbx`.

Common commands:

```sh
make manage id=example             # browse accounts, passwords, and profile data
make pass-set id=example title=GitHub
make pass id=example title=GitHub
make ai id=example
make organize id=example           # migrate an older vault layout
make doctor id=example
make backup
```

## Vault layout

The project uses ordinary KeePass groups. `Passwords` is not required.

```text
ID/
├── ID_info       profile fields and identity information
├── ID_account    website, cloud, and application accounts
├── ID_emails     email accounts and their protected recovery fields
└── ID_events     important changes and reminders
```

Accounts and passwords share one native entry. Recovery codes, API keys, and similar values are protected custom fields. Files belong in the corresponding KeePass entry as native attachments. Set KeePassXC-Browser's new-credentials group to `ID/ID_account` manually; ID does not change browser-plugin settings.

## Security model

KeePassXC performs KDBX encryption and merging. ID keeps exported XML in Linux anonymous memory, writes through a temporary encrypted transaction, verifies the result, and leaves an encrypted backup before replacement. It never sends passwords, master passwords, protected values, or attachment contents to AI services.

Do not open the same vault for editing on two devices at once. Wait for Syncthing to finish before switching devices. See the maintenance guide for conflict handling and backups.

## Documentation

- [Documentation index](docs/README.md)
- [English guide](docs/en/guide.md)
- [English maintenance guide](docs/en/maintenance.md)
- [中文使用说明](docs/zh-CN/GUIDE.md)
- [中文维护文档](docs/zh-CN/MAINTENANCE.md)
- [Changelog](CHANGELOG.md)

## Development

Development happens on `dev`. Run:

```sh
make test
make check
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

Identity vaults, backups, logs, exports, and secret configuration are local data and are excluded from Git.

## License

ID is available under the [MIT License](LICENSE).

# User guide

## Requirements

ID currently targets Linux and needs Rust 1.89+, GNU Make, and `keepassxc-cli`. KeePassXC remains the component that encrypts, decrypts, and merges KDBX files.

## Create and open an identity

```sh
make new id=example
make manage id=example
```

Each identity has an independent master password. New vaults contain the native group `ID/ID_info/example`. Use the full-screen TUI to add accounts, profile fields, email accounts, events, protected fields, and attachments.

## Native vault layout

```text
ID/
├── ID_info
├── ID_account
├── ID_emails
└── ID_events
```

Accounts and passwords are stored in the same entry. Recovery codes and API keys use protected custom fields. Attachments stay with their KeePass entry. `Passwords` is an optional user group, not a project requirement.

Set the KeePassXC-Browser new-credentials group to `ID/ID_account` in the extension settings. ID does not change browser-plugin configuration.

## Common operations

```sh
make manage id=example
make pass-set id=example title=GitHub
make pass id=example title=GitHub
make pass-set id=example title=Cloudflare field=RecoveryCode
make ai id=example
make organize id=example
```

AI receives local metadata for read-only queries. Passwords, master passwords, protected values, and attachment contents stay local. Use KeePassXC or the TUI for sensitive values.

## Multiple devices

Close KeePassXC and ID before switching devices. Wait until Syncthing reports the same complete KDBX version on the next device. Never edit one vault concurrently on two devices.

For migration, conflict handling, backups, permissions, and Syncthing handoff, read the [maintenance guide](maintenance.md).

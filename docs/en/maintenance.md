# Maintenance and operations

## Install and check

Install Rust 1.89+, GNU Make, and KeePassXC. Then run:

```sh
make build
make check
```

`make check` verifies repository rules, local permissions, ignored paths, and identity-directory health. It does not decrypt vaults.

## Backups and migration

```sh
make backup
make organize id=example
```

`make organize` previews and confirms migration from older project groups such as `Passwords/ID`, old `ID_info`, `ID_emails`, and `ID/账号`. It preserves UUIDs, protected fields, passwords, attachments, and history, creates an encrypted backup before writing, and aborts on conflicts. It does not delete empty groups or `Passwords`.

## Syncthing

Use `make settings` to configure optional local Syncthing handoff checks. The API key is stored in local secret configuration. Before opening or changing a vault, ID can wait for the selected device to hold the complete matching KDBX version. A timeout, conflict, or offline peer stops the operation instead of claiming success.

Use `make doctor id=example` for a read-only scan. `make doctor id=example fix=1` can archive confirmed encrypted conflict copies after local confirmation. It does not merge copies or delete unknown files.

## Sensitive data

Passwords and protected fields are entered locally and never sent to AI services or written to logs. Do not put KDBX files, backups, exports, logs, or secret configuration under Git. See the Chinese guide for the full terminal and attachment workflows.

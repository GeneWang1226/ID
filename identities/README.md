# Identity vaults

This directory contains one KeePassXC KDBX vault per identity, for example `gene.kdbx` or `work.kdbx`.

## Syncthing

Configure the project directory `ID/` as the Syncthing folder. The root `.stignore` then keeps source code, local configuration, logs, backups, and exports out of synchronization while allowing this directory's encrypted vaults to sync.

Do not configure only `identities/` unless you also reproduce the same ignore and operational rules manually. Close KeePassXC and ID before switching devices, wait for Syncthing to finish, and never edit one KDBX on two devices at the same time.

## Git

Git tracks this README only. All `*.kdbx` files and other identity data remain ignored. Create identities with `make new id=<name>`; do not commit passwords, recovery codes, exports, or temporary files.

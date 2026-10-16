# Rebuilding from nothing

What to do if the machine running Endora is wiped, and what makes it possible. The decisions
behind this are [ADR 0098](adr/0098-it-can-be-rebuilt.md). Read it once now, not on the day:
the steps assume a few things were put in place beforehand (below).

## What holds what

| What | Where it lives | How it comes back |
|---|---|---|
| Endora's settings, keys and memory | `endora.db` in the node's data folder (a Docker volume by default) | Endora's own nightly copy, `backups/daily-*.db` beside it, restored from the house's backup |
| The rebuild kit: your ansible-nas checkout, with the vault encrypted | its checkout folder on the NAS | the house's backup, with the home folder |
| The vault password | your password manager, and `vault_password.txt.enc` (encrypted under your recovery passphrase) in the backup | either one |
| The house's backup repository password | your password manager | needed to read any backup |
| The mail copy | the mail mirror's folder | re-copied from the mail provider by the `mail_mirror` role |
| Local models | Ollama's folder | pulled again (`ollama pull`) |
| A handheld's pairing | the handheld | paired again from Endora |

## Put in place beforehand

1. **Endora's nightly copies.** Nothing to do: the node writes `backups/daily-YYYY-MM-DD.db`
   at 01:30 every night and `weekly-YYYY-MM-DD.db` on Sundays, keeping seven of each, and says
   so in Activity. If Activity says a copy failed, find out why that day.
2. **The house's backup covers the data folder**, after 01:30. If the node's data is in a
   Docker volume, the backup must include Docker's volumes folder; with `ENDORA_DATA_DIR`, that
   folder.
3. **The ansible-nas checkout is backed up**, ideally **without** the plain
   `vault_password.txt` (exclude it from the plan that covers the checkout).
4. **The vault password is kept twice**, never only beside the vault:
   - in your password manager, and
   - encrypted under a recovery passphrase you choose and remember:

     ```sh
     mkdir -p ~/ansible-nas-recovery && chmod 700 ~/ansible-nas-recovery
     ansible-vault encrypt --vault-id recovery@prompt \
       ~/ansible-nas/vault_password.txt --output ~/ansible-nas-recovery/vault_password.txt.enc
     ```

     Keep the copy outside the checkout so it can never be committed, in a folder the backup
     covers.
5. **The backup repository's own password is in your password manager.**
6. **An offsite copy** (a second backup repository off the machine). Without one, losing the
   machine loses its backups too.

## Rebuild

1. **The machine.** Install the OS, mount the disks, install Docker. If the disk holding the
   backups survived, it is your source; otherwise the offsite copy.
2. **The rebuild kit first.** With restic directly (Backrest is not running yet), restore the
   ansible-nas checkout and the recovery folder from the latest snapshot:

   ```sh
   restic -r <backup repository> snapshots --path <checkout path>
   restic -r <backup repository> restore latest --path <checkout path> --target /
   ```

   Paths in a snapshot are as the backup service saw them: Backrest mounts the machine under
   `/sources/`, so the checkout is `/sources/home/<you>/ansible-nas` there. Restore to a scratch
   target and move it into place if the layout differs.

3. **The vault password.** Copy it from your password manager into
   `~/ansible-nas/vault_password.txt` (mode 600), or decrypt the backed-up copy:

   ```sh
   ansible-vault decrypt --vault-id recovery@prompt \
     ~/ansible-nas-recovery/vault_password.txt.enc --output ~/ansible-nas/vault_password.txt
   ```

4. **The house.** Run the playbook as you normally do. Every service, including the backup
   service and the mail mirror, comes back with its settings from the vault.
5. **Endora's data.** Restore the newest `backups/daily-*.db` from the backup, then, with the
   node stopped, put it in place as the database:

   ```sh
   docker compose stop node
   cp daily-YYYY-MM-DD.db <data folder>/endora.db
   docker compose start node
   ```

   Then deploy as usual (`make deploy`). A copy is a whole database, checked when written: the
   node opens it as it would its own.
6. **Check.** The console opens and signs you in with your usual password and authenticator
   code. Settings → Connections shows your services set up, Skills shows them on, About you
   shows what Endora knew, and Activity shows the work up to the night of the copy.
7. **The handheld.** Pair it again.

## Proving it

A backup nobody has restored is a hope. When declared configuration lands, a drill restores a
copy and the declared configuration into a scratch node and checks it comes up whole; until
then, open a copy now and then:

```sh
sqlite3 daily-YYYY-MM-DD.db 'PRAGMA integrity_check;'
```

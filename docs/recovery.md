# Rebuilding from nothing

What to do if the machine running Endora is wiped, and what makes it possible. The decisions
behind this are [ADR 0098](adr/0098-it-can-be-rebuilt.md). Read it once now, not on the day:
the steps assume a few things were put in place beforehand (below).

## What holds what

| What | Where it lives | How it comes back |
|---|---|---|
| Endora's settings, keys and memory | `endora.db` in the node's data folder (a Docker volume by default) | Endora's own nightly copy, `backups/daily-*.db` beside it, restored from the house's backup |
| HQ's goals, tasks and desk | `hq.db` in HQ's data folder (its own Docker volume) | HQ's own nightly copy, `backups/daily-*.db` beside it, restored from the house's backup |
| Endora's secrets key, which the credentials in `endora.db` are sealed under ([ADR 0125](adr/0125-secrets-at-rest-in-the-node.md)) | `secrets.key` in the node's data folder | your password manager — **never** only the house's backup beside the database |
| The rebuild kit: your ansible-nas checkout, with the vault encrypted | its checkout folder on the NAS | the house's backup, with the home folder |
| The Ansible vault password | your password manager, and `vault_password.txt.enc` (encrypted under your recovery passphrase) in the backup | either one |
| The house's backup repository password | your password manager | needed to read any backup |
| The mail copy | the mail mirror's folder | re-copied from the mail provider by the `mail_mirror` role |
| Local models | Ollama's folder | pulled again (`ollama pull`) |
| A handheld's pairing | the handheld | paired again from Endora |

## Put in place beforehand

1. **Both stores' nightly copies.** Nothing to do: the node (`endora.db`) and HQ (`hq.db`)
   each write `backups/daily-YYYY-MM-DD.db` beside their database at 01:30 every night and
   `weekly-YYYY-MM-DD.db` on Sundays, keeping seven of each. **Each copy is proven by restore**
   before it is kept: it is restored to a scratch file (in a `.scratch` folder inside `backups/`,
   because HQ's container has a read-only root; it is emptied after each proof) and must open whole, have the live
   database's schema and hold rows wherever the live database does. A copy that fails is
   deleted and tried again on the next ten-minute tick, up to three times a night (the count
   restarts if the service restarts). A copy is flushed to disk before it gets its `daily-` name. The node
   says in Activity, "proven by restore" when it is, and once if all three failed; HQ says so
   in its log (`docker logs`). If a copy failed, find out why that day.
   Release-time `before-*-hq` copies of HQ's store are **no longer automatic** with the compose
   bundle (the old release script that made them is gone): take one by hand before an upgrade
   you doubt (`docker compose exec hq …`, or copy the volume's `hq.db` while HQ is stopped).
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
6. **Endora's secrets key is in your password manager**, and excluded from the plan that
   covers the data folder (`docs/homelab.md`, "Secrets at rest"):
   `docker compose exec node base64 /data/secrets.key`. A database copy without it restores
   no credential.
7. **An offsite copy** (a second backup repository off the machine). Without one, losing the
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
   # the secrets key, from your password manager (paste, then Ctrl-D):
   base64 -d > <data folder>/secrets.key && chmod 600 <data folder>/secrets.key
   docker compose start node
   ```

   Without the key the node does not start, and says the key file is missing.

   Then deploy as usual (`make deploy`). A copy is a whole database, proven by restore when it
   was written: the node opens it as it would its own. HQ's store comes back the same way: stop
   HQ, put the newest `backups/daily-*.db` from HQ's data folder in place as `hq.db`, start it.
6. **Check.** The console opens and signs you in with your usual password and authenticator
   code. Settings → Connections shows your services set up, Skills shows them on, About you
   shows what Endora knew, and Activity shows the work up to the night of the copy.
7. **The handheld.** Pair it again.

## Proving it

A backup nobody has restored is a hope, so every nightly copy of both stores is restored to a
scratch file and checked before it is kept (above). You can run the same proof on any copy,
inside the container so it sees the right database, without starting a server:

```sh
docker compose exec node endora-node verify-backup /data/backups/daily-YYYY-MM-DD.db
```

It restores the copy into a `.scratch` folder beside it, so the folder must be writable. It exits 0 when the copy restores and non-zero, with the reason, when it does not. The proof
compares the copy with a live database: `ENDORA_DB` by default, or the file named as a second
argument, which is how HQ's copy is checked (`endora-node verify-backup <hq copy> <hq.db>`,
with both files visible to the command, for example in a one-off container with HQ's volume
mounted read-only). The proof reads "live has rows" at the moment it runs, so run it soon
after a copy is taken. It does not cover `secrets.key`, which is not in any copy; a drill that
restores a copy into a scratch node runs when declared configuration lands.

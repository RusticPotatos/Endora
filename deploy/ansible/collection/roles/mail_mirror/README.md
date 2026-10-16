# Mail Mirror

A read-only copy of your mail on the NAS, as a Maildir, for a local assistant to read. It
copies; it never sends, deletes, moves or marks anything.

- **Gmail** can be read two ways. Through the **Gmail API** with a `gmail.readonly` token
  (`kind: gmail`), Google itself refuses anything but reading, whatever the mirror asked for;
  this needs a published Google Cloud app (below). Or over **IMAP with an app password**
  (`kind: gmail_imap`), the way Home Assistant reads Gmail: no Google Cloud project, no
  expiry, but the password could do more than read, so the mirror's own code (below) is what
  keeps it to reading. Either way a message gets the same name, so switching between them
  copies nothing twice.
- **iCloud** has no such API and is read over IMAP with an app-specific password. Every
  folder is opened read-only (`EXAMINE`) and every message fetched with `BODY.PEEK`, which
  does not mark it as read. No command that changes a mailbox is in the script, and its tests
  fail if one is added.
- It starts with the last **week** of mail and widens one step a day (2, 4, 8, 16, 32 weeks)
  to a **year**, so the history arrives gradually.
- It runs in its own container on its own network. The `docker_egress` role lets that network
  reach the internet on HTTPS and IMAPS only, plus DNS. Your LAN, other containers and the NAS
  itself are unreachable from it. Which hosts it talks to (Google, Apple) is fixed in its
  code, not configurable.
- Its logs say how many messages were copied, never a sender or a subject.
- The copy is stored in `mail_mirror_root` (default `/srv/mail-mirror`), **outside**
  `docker_home`, so it is not in your backups: the provider already has the mail, and a second
  copy in every snapshot is only risk. Owned by its own system user, not readable by others.

## Gmail with an app password (`kind: gmail_imap`)

With 2-Step Verification on, create an app password at
[myaccount.google.com/apppasswords](https://myaccount.google.com/apppasswords), named for the
mirror. Make a new one rather than reusing another app's, so each can be revoked on its own.
The mirror reads "All Mail" (everything but Spam and Trash, sent mail included).

## Gmail through the API: a read-only token, once (`kind: gmail`)

You do this yourself; nobody else needs to see your sign-in.

1. In the [Google Cloud console](https://console.cloud.google.com/), create a project.
2. Enable the **Gmail API** for it.
3. Under **OAuth consent screen**, choose *External*, fill in the app name and your own
   address, add the scope `https://www.googleapis.com/auth/gmail.readonly`, add your Gmail
   addresses as test users, and then **publish the app** ("In production"). In *Testing*,
   Google expires the token after 7 days and the mirror would quietly stop. An unverified
   app used only by you works; the consent screen will warn that it is unverified.
4. Under **Credentials**, create an **OAuth client ID** of type *Desktop app*. Note the
   client ID and secret.
5. On your own computer, for each Gmail account:

   ```bash
   python3 roles/mail_mirror/files/gmail_consent.py CLIENT_ID CLIENT_SECRET
   ```

   Sign in as that account and allow read access. The script refuses a token that is not
   exactly read-only, and writes it to `gmail-refresh-token.txt` (mode 600). Put it in your
   vault and delete the file.

## iCloud: an app-specific password

At [account.apple.com](https://account.apple.com/), under *Sign-In and Security*, create an
app-specific password for the mirror.

## Settings

In your vault (never in a tracked file; this repository may be public):

```yaml
vault_mail_mirror_gmail_user: "..."
vault_mail_mirror_gmail_app_password: "..."
vault_mail_mirror_gmail_client_id: "..."
vault_mail_mirror_gmail_client_secret: "..."
vault_mail_mirror_personal_refresh_token: "..."
vault_mail_mirror_icloud_user: "..."
vault_mail_mirror_icloud_app_password: "..."
```

And in your `group_vars`:

```yaml
mail_mirror_enabled: true
mail_mirror_accounts:
  - name: personal
    kind: gmail_imap
    user: "{{ vault_mail_mirror_gmail_user }}"
    app_password: "{{ vault_mail_mirror_gmail_app_password }}"
  - name: another
    kind: gmail
    client_id: "{{ vault_mail_mirror_gmail_client_id }}"
    client_secret: "{{ vault_mail_mirror_gmail_client_secret }}"
    refresh_token: "{{ vault_mail_mirror_personal_refresh_token }}"
  - name: icloud
    kind: icloud
    user: "{{ vault_mail_mirror_icloud_user }}"
    app_password: "{{ vault_mail_mirror_icloud_app_password }}"
```

More Gmail accounts reuse the same client ID and secret, each with its own refresh token and
name. Then:

```bash
ansible-playbook -i inventories/<your_inventory>/inventory nas.yml --tags mail_mirror --ask-become-pass
```

## Checking it

```bash
docker logs --tail 5 mail-mirror        # "personal: 42 new, window 7 days; ..."
sudo ls /srv/mail-mirror/maildir        # one folder per account
sudo iptables -S EG-MAILMIRROR          # the egress rules
```

To confirm the network is locked down, from a throwaway container on the same network
(replace the address with something on your LAN):

```bash
docker run --rm --network mail_mirror alpine:3.20 sh -c \
  'wget -qT5 -O- http://<a LAN address> >/dev/null && echo REACHED || echo blocked'
```

It should say `blocked`.

## Turning it off

`mail_mirror_enabled: false` and run the same tag again. The container and its egress rules
are removed; the copied mail stays in `mail_mirror_root` until you delete it.

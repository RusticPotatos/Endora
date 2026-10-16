use super::*;

const PLAIN: &str = "Delivered-To: john@example.com\r
Authentication-Results: mx.example.com; dkim=pass header.i=@example.org; spf=pass smtp.mailfrom=example.org; dmarc=pass (p=REJECT) header.from=example.org\r
From: Jane Doe <Jane@Example.ORG>\r
Reply-To: jane.doe@example.org\r
To: John Doe <john@example.com>\r
Cc: bob@example.net\r
Subject: Lunch on Thursday?\r
Date: Tue, 14 Oct 2026 09:30:00 -0400\r
Message-ID: <abc123@example.org>\r
In-Reply-To: <sent-1@example.com>\r
References: <sent-0@example.com> <sent-1@example.com>\r
\r
Are you free Thursday? Menu: https://cafe.example.net/menu\r
";

const HTML_BILL: &str = "Delivered-To: john@example.com\r
Authentication-Results: mx.example.com; spf=softfail smtp.mailfrom=billing.example.net; dkim=none; dmarc=fail\r
From: Example Dental Billing <billing@example.net>\r
To: john@example.com\r
Subject: Your statement\r
Date: Wed, 15 Oct 2026 08:00:00 +0000\r
Message-ID: <bill-9@example.net>\r
List-Unsubscribe: <mailto:unsub@example.net>\r
MIME-Version: 1.0\r
Content-Type: multipart/mixed; boundary=\"b1\"\r
\r
--b1\r
Content-Type: text/html; charset=utf-8\r
\r
<p>Amount due: $180.00</p><a href=\"https://example.net@example.org/x?y=1\">Pay at example.net</a> <A HREF='https://example.net/account'>Your account</A>\r
--b1\r
Content-Type: application/pdf; name=\"statement.pdf\"\r
Content-Disposition: attachment; filename=\"statement.pdf\"\r
Content-Transfer-Encoding: base64\r
\r
JVBERi0xLjQK\r
--b1\r
Content-Type: text/calendar; method=REQUEST\r
\r
BEGIN:VCALENDAR\r
END:VCALENDAR\r
--b1--\r
";

#[test]
fn a_plain_message_gives_up_its_facts() {
    let m = parse("personal/1", "personal", PLAIN.as_bytes()).unwrap();
    assert_eq!(m.from_address, "jane@example.org");
    assert_eq!(m.from_name, "Jane Doe");
    assert_eq!(m.reply_to.as_deref(), Some("jane.doe@example.org"));
    assert_eq!(m.to, ["john@example.com"]);
    assert_eq!(m.cc, ["bob@example.net"]);
    assert_eq!(m.delivered_to.as_deref(), Some("john@example.com"));
    assert_eq!(m.subject, "Lunch on Thursday?");
    assert_eq!(m.message_id, "<abc123@example.org>");
    assert_eq!(
        m.replies_to,
        ["<sent-0@example.com>", "<sent-1@example.com>"]
    );
    assert_eq!(m.at_ms, 1_791_984_600_000, "09:30 at UTC-4 is 13:30 UTC");
    assert!(!m.mailing_list && !m.invitation);
    assert_eq!(
        m.auth,
        Auth {
            spf: Verdict::Pass,
            dkim: Verdict::Pass,
            dmarc: Verdict::Pass
        }
    );
}

#[test]
fn links_are_recorded_and_never_left_in_the_text() {
    let m = parse("personal/1", "personal", PLAIN.as_bytes()).unwrap();
    assert!(!m.text.contains("http"), "{}", m.text);
    assert!(m.text.contains("[link]"));
    assert_eq!(
        m.links
            .iter()
            .map(|l| l.domain.as_str())
            .collect::<Vec<_>>(),
        ["cafe.example.net"]
    );
}

#[test]
fn a_link_is_recorded_by_where_it_really_goes() {
    let m = parse("personal/2", "personal", HTML_BILL.as_bytes()).unwrap();
    let domains: Vec<(&str, &str)> = m
        .links
        .iter()
        .map(|l| (l.domain.as_str(), l.text.as_str()))
        .collect();
    assert!(
        domains.contains(&("example.org", "Pay at example.net")),
        "user@host must not hide the real host: {domains:?}"
    );
    assert!(
        domains.contains(&("example.net", "Your account")),
        "{domains:?}"
    );
}

#[test]
fn a_bulk_bill_with_an_attachment_and_an_invitation() {
    let m = parse("personal/2", "personal", HTML_BILL.as_bytes()).unwrap();
    assert!(m.mailing_list);
    assert!(m.invitation);
    assert_eq!(m.attachments.len(), 1);
    assert_eq!(m.attachments[0].name, "statement.pdf");
    assert_eq!(m.attachments[0].mime, "application/pdf");
    assert_eq!(
        m.auth,
        Auth {
            spf: Verdict::Fail,
            dkim: Verdict::Unknown,
            dmarc: Verdict::Fail
        }
    );
    assert!(m.text.contains("180.00"), "{}", m.text);
}

#[test]
fn hosts_are_read_the_way_a_browser_reads_them() {
    assert_eq!(
        host_of("https://a.example.com/x").as_deref(),
        Some("a.example.com")
    );
    assert_eq!(
        host_of("https://example.com@example.net/").as_deref(),
        Some("example.net")
    );
    assert_eq!(
        host_of("http://Example.COM:8080/?q=1").as_deref(),
        Some("example.com")
    );
    assert_eq!(host_of("javascript:alert(1)"), None);
    assert_eq!(host_of("https://localhost/"), None);
}

#[test]
fn something_that_is_not_mail_is_none() {
    assert!(parse("k", "a", b"").is_none());
}

// ---- the copy ----

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("endora-mail-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn keys_are_plain_names_inside_the_copy() {
    let root = scratch("keys");
    for (path, body) in [
        ("personal/new/a", PLAIN),
        ("personal/cur/b", PLAIN),
        ("personal/tmp/c", PLAIN),
        ("personal/new/.hidden", PLAIN),
        (".secret/new/d", PLAIN),
        ("icloud/new/e", PLAIN),
    ] {
        let p = root.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }
    let copy = MaildirCopy::new(&root);
    assert_eq!(copy.keys(), ["icloud/e", "personal/a", "personal/b"]);
    assert!(copy.read("personal/b").is_some());
    // A real message just outside the account folders: a key that climbs to it must not reach
    // it, or one that climbs further could reach anything the node can read.
    std::fs::write(root.join("outside"), PLAIN).unwrap();
    assert!(
        copy.read("personal/../../outside").is_none(),
        "a key climbed out of the copy"
    );
    assert!(copy.read("personal/../../etc/passwd").is_none());
    assert!(copy.read("../personal/a").is_none());
    assert!(copy.read("personal/tmp-not-there").is_none());
    std::fs::remove_dir_all(root).unwrap();
}

// ---- the store ----

fn store() -> MailStore {
    let db = Db::open_in_memory().unwrap();
    migrate(&db).unwrap();
    migrate(&db).unwrap(); // idempotent
    MailStore::new(db)
}

#[test]
fn indexing_twice_is_once_and_the_headline_comes_back_whole() {
    let s = store();
    let m = parse("personal/1", "personal", PLAIN.as_bytes()).unwrap();
    s.index(&m).unwrap();
    s.index(&m).unwrap();
    assert_eq!(s.known_keys().unwrap().len(), 1);
    assert_eq!(s.headlines().unwrap(), [m.headline()]);
}

#[test]
fn unread_mail_comes_newest_first_and_leaves_once_read_or_passed_over() {
    let s = store();
    for (key, at) in [("a/1", 100), ("a/2", 300), ("a/3", 200), ("a/old", 10)] {
        s.index(&Message {
            key: key.into(),
            account: "a".into(),
            at_ms: at,
            ..Message::default()
        })
        .unwrap();
    }
    assert_eq!(s.unread_since(50, 10).unwrap(), ["a/2", "a/3", "a/1"]);
    assert_eq!(s.unread_since(50, 2).unwrap(), ["a/2", "a/3"]);
    s.pass_over("a/2", 1).unwrap();
    s.save(
        &Digest {
            key: "a/3".into(),
            at_ms: 200,
            from_address: "x@example.net".into(),
            from_name: "X".into(),
            bucket: Bucket::Money,
            reasons: vec![Reason::MoneyDue],
            sentence: Some("A bill.".into()),
            flags: every_flag(),
            read_ok: true,
            cautions: vec!["the first message from this sender".into()],
            appointment: Some(("2026-10-02".into(), "Dental checkup".into())),
        },
        2,
    )
    .unwrap();
    assert_eq!(s.unread_since(50, 10).unwrap(), ["a/1"]);
    assert_eq!(s.progress(50).unwrap(), (4, 1));
    let digests = s.digests_since(0).unwrap();
    assert_eq!(digests.len(), 1, "a message passed over has no digest");
    let d = &digests[0];
    assert_eq!(
        (d.bucket, d.reasons.as_slice()),
        (Bucket::Money, &[Reason::MoneyDue][..])
    );
    // Every flag survives storage: one left out of either side is lost at the next read.
    assert_eq!(d.flags, every_flag());
    assert!(d.read_ok);
    assert_eq!(d.cautions, ["the first message from this sender"]);
    assert_eq!(
        d.appointment,
        Some(("2026-10-02".to_owned(), "Dental checkup".to_owned()))
    );
}

/// Every flag set, so a flag storage forgets shows as a difference.
fn every_flag() -> Flags {
    Flags {
        money_due: true,
        amount: Some("180.00".into()),
        account_change: true,
        one_time_code: true,
        order_problem: true,
        app_needs_you: true,
        needs_reply: true,
        dates: vec![("2026-10-20".into(), "due".into())],
    }
}

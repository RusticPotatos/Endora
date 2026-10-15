use super::*;

const DAY: i64 = 24 * 60 * 60 * 1000;
const OWNER: &str = "john@example.com";

fn headline(
    from: &str,
    to: &[&str],
    id: &str,
    replies: &[&str],
    delivered: Option<&str>,
) -> Headline {
    Headline {
        key: format!("k-{id}"),
        at_ms: 1_000,
        from_address: from.to_owned(),
        recipients: to.iter().map(|s| (*s).to_owned()).collect(),
        delivered_to: delivered.map(str::to_owned),
        message_id: id.to_owned(),
        replies_to: replies.iter().map(|s| (*s).to_owned()).collect(),
    }
}

/// A mailbox where the owner has written to Jane once (and started a thread), and has had a
/// few messages from elsewhere.
fn mailbox() -> Mailbox {
    let mut all = vec![headline(
        OWNER,
        &["jane@example.org"],
        "<sent-1@example.com>",
        &["<from-jane-0@example.org>"],
        None,
    )];
    for n in 0..5 {
        all.push(headline(
            "news@example.net",
            &[OWNER],
            &format!("<n{n}>"),
            &[],
            Some(OWNER),
        ));
    }
    Mailbox::learn(&all)
}

fn message(from: &str) -> Message {
    Message {
        key: "personal/1".to_owned(),
        account: "personal".to_owned(),
        at_ms: 10 * DAY,
        from_address: from.to_owned(),
        from_name: "Someone".to_owned(),
        to: vec![OWNER.to_owned()],
        delivered_to: Some(OWNER.to_owned()),
        message_id: "<m1@example.org>".to_owned(),
        ..Message::default()
    }
}

// ---- the mailbox ----

#[test]
fn the_owner_is_where_the_mail_is_delivered() {
    let mb = mailbox();
    assert_eq!(mb.owners.iter().collect::<Vec<_>>(), [OWNER]);
}

#[test]
fn a_stray_delivered_to_is_not_an_owner() {
    let mut all: Vec<Headline> = (0..40)
        .map(|n| {
            headline(
                "a@example.net",
                &[OWNER],
                &format!("<{n}>"),
                &[],
                Some(OWNER),
            )
        })
        .collect();
    all.push(headline(
        "b@example.net",
        &["list@example.net"],
        "<x>",
        &[],
        Some("list@example.net"),
    ));
    assert_eq!(Mailbox::learn(&all).owners.len(), 1);
}

#[test]
fn whom_the_owner_writes_to_and_which_threads_are_theirs_are_learned_from_what_they_sent() {
    let mb = mailbox();
    assert!(mb.written_to.contains("jane@example.org"));
    assert!(mb.sent_ids.contains("<sent-1@example.com>"));
    assert!(mb.answered.contains("<from-jane-0@example.org>"));
    assert!(!mb.written_to.contains("news@example.net"));
}

#[test]
fn a_sending_domain_is_counted_from_its_first_message() {
    let mb = mailbox();
    assert_eq!(mb.domains.get("example.net"), Some(&(1_000, 5)));
}

// ---- buckets ----

#[test]
fn an_invitation_is_an_appointment_whatever_the_reader_says() {
    let mut m = message("someone@example.net");
    m.invitation = true;
    assert_eq!(
        bucket(&m, &mailbox(), Some(Bucket::Newsletters)),
        Bucket::Appointments
    );
}

#[test]
fn someone_the_owner_writes_to_is_a_person_whatever_the_reader_says() {
    let m = message("jane@example.org");
    assert_eq!(
        bucket(&m, &mailbox(), Some(Bucket::Updates)),
        Bucket::People
    );
}

#[test]
fn a_reply_in_the_owners_thread_is_from_a_person() {
    let mut m = message("someone-new@example.org");
    m.replies_to = vec!["<sent-1@example.com>".to_owned()];
    assert_eq!(bucket(&m, &mailbox(), None), Bucket::People);
}

#[test]
fn a_list_message_is_a_newsletter_unless_it_is_something_a_company_really_sends() {
    let mut m = message("news@example.net");
    m.mailing_list = true;
    assert_eq!(
        bucket(&m, &mailbox(), Some(Bucket::Other)),
        Bucket::Newsletters
    );
    assert_eq!(bucket(&m, &mailbox(), None), Bucket::Newsletters);
    assert_eq!(
        bucket(&m, &mailbox(), Some(Bucket::Money)),
        Bucket::Money,
        "bills come from bulk senders"
    );
    assert_eq!(
        bucket(&m, &mailbox(), Some(Bucket::People)),
        Bucket::Newsletters,
        "a list message cannot be passed off as a person"
    );
}

#[test]
fn a_list_message_from_someone_the_owner_writes_to_is_still_a_list_message() {
    let mut m = message("jane@example.org");
    m.mailing_list = true;
    assert_eq!(bucket(&m, &mailbox(), None), Bucket::Newsletters);
}

#[test]
fn otherwise_the_readers_suggestion_stands_and_nothing_means_other() {
    let m = message("billing@example.net");
    assert_eq!(bucket(&m, &mailbox(), Some(Bucket::Money)), Bucket::Money);
    assert_eq!(bucket(&m, &mailbox(), None), Bucket::Other);
}

// ---- importance ----

fn flags() -> Flags {
    Flags::default()
}

#[test]
fn newsletters_and_the_owners_own_mail_are_never_important() {
    let mut f = flags();
    f.money_due = true;
    let m = message("news@example.net");
    assert!(why_important(&m, &mailbox(), Bucket::Newsletters, &f, 20 * DAY).is_empty());
    let mine = message(OWNER);
    assert!(why_important(&mine, &mailbox(), Bucket::Money, &f, 20 * DAY).is_empty());
}

#[test]
fn a_person_the_owner_writes_to_matters_and_waiting_three_days_matters_more() {
    let mut m = message("jane@example.org");
    let mut f = flags();
    let mb = mailbox();
    assert_eq!(
        why_important(&m, &mb, Bucket::People, &f, m.at_ms + DAY),
        [Reason::FromSomeoneYouWriteTo]
    );
    f.needs_reply = true;
    assert_eq!(
        why_important(&m, &mb, Bucket::People, &f, m.at_ms + 3 * DAY),
        [Reason::FromSomeoneYouWriteTo, Reason::WaitingOnYou]
    );
    // Answered: no longer waiting.
    m.message_id = "<from-jane-0@example.org>".to_owned();
    assert_eq!(
        why_important(&m, &mb, Bucket::People, &f, m.at_ms + 9 * DAY),
        [Reason::FromSomeoneYouWriteTo]
    );
}

#[test]
fn a_stranger_is_not_important_for_being_a_person() {
    let m = message("stranger@example.net");
    let mut f = flags();
    f.needs_reply = true;
    assert!(why_important(&m, &mailbox(), Bucket::People, &f, 99 * DAY).is_empty());
}

#[test]
fn money_security_appointments_orders_and_the_app_matter_when_they_should() {
    let m = message("billing@example.net");
    let mb = mailbox();
    let mut f = flags();
    assert!(
        why_important(&m, &mb, Bucket::Money, &f, 0).is_empty(),
        "a receipt asks for nothing"
    );
    f.money_due = true;
    assert_eq!(
        why_important(&m, &mb, Bucket::Money, &f, 0),
        [Reason::MoneyDue]
    );
    f.account_change = true;
    assert_eq!(
        why_important(&m, &mb, Bucket::Security, &f, 0),
        [Reason::AccountSecurity]
    );
    assert_eq!(
        why_important(&m, &mb, Bucket::Appointments, &flags(), 0),
        [Reason::Appointment]
    );
    f.order_problem = true;
    assert_eq!(
        why_important(&m, &mb, Bucket::Orders, &f, 0),
        [Reason::OrderProblem]
    );
    assert_eq!(
        why_important(&m, &mb, Bucket::YourApp, &flags(), 0),
        [Reason::YourApp]
    );
    let mut list = m.clone();
    list.mailing_list = true;
    assert!(why_important(&list, &mb, Bucket::YourApp, &flags(), 0).is_empty());
}

// ---- what is accepted from the reader ----

const TODAY: i64 = 20_000; // a day number

fn day(n: i64) -> String {
    // 20_000 days after 1970-01-01 is 2024-10-04.
    let base = days_of("2024-10-04").unwrap();
    assert_eq!(base, TODAY);
    let target = base + n;
    // Walk forward/back from the base by whole days.
    let mut d = (2024, 10, 4);
    let step = |d: &mut (i64, i64, i64), forward: bool| loop {
        if forward {
            d.2 += 1
        } else {
            d.2 -= 1
        }
        if d.2 > 31 {
            d.2 = 1;
            d.1 += 1
        }
        if d.2 < 1 {
            d.2 = 31;
            d.1 -= 1
        }
        if d.1 > 12 {
            d.1 = 1;
            d.0 += 1
        }
        if d.1 < 1 {
            d.1 = 12;
            d.0 -= 1
        }
        if days_of(&format!("{:04}-{:02}-{:02}", d.0, d.1, d.2)).is_some() {
            break;
        }
    };
    while days_of(&format!("{:04}-{:02}-{:02}", d.0, d.1, d.2)).unwrap() != target {
        step(&mut d, target > base);
    }
    format!("{:04}-{:02}-{:02}", d.0, d.1, d.2)
}

#[test]
fn a_good_reply_is_read_whole() {
    let raw = format!(
        r#"{{"bucket":"money","sentence":"Your dental bill for the October visit is due.","money_due":true,"amount":"$1,180.5","dates":[{{"date":"{}","what":"payment due"}}]}}"#,
        day(10)
    );
    let (b, s, f) = read_reply(&raw, TODAY).unwrap();
    assert_eq!(b, Some(Bucket::Money));
    assert_eq!(
        s.as_deref(),
        Some("Your dental bill for the October visit is due.")
    );
    assert!(f.money_due && !f.account_change);
    assert_eq!(f.amount.as_deref(), Some("1180.50"));
    assert_eq!(f.dates, [(day(10), "payment due".to_owned())]);
}

#[test]
fn a_reply_wrapped_in_chatter_is_still_read() {
    let (b, ..) = read_reply("Sure! {\"bucket\":\"updates\"} hope that helps", TODAY).unwrap();
    assert_eq!(b, Some(Bucket::Updates));
}

#[test]
fn not_json_is_nothing() {
    assert!(read_reply("I could not read that", TODAY).is_none());
    assert!(read_reply("{not json}", TODAY).is_none());
}

#[test]
fn wrong_shapes_are_dropped_field_by_field() {
    let raw = r#"{"bucket":"urgent!!","sentence":42,"money_due":"yes","amount":"lots","dates":"tomorrow"}"#;
    let (b, s, f) = read_reply(raw, TODAY).unwrap();
    assert_eq!((b, s), (None, None));
    assert_eq!(f, Flags::default());
}

#[test]
fn dates_must_be_real_and_near() {
    let raw = format!(
        r#"{{"dates":[{{"date":"2025-02-30","what":"x"}},{{"date":"{}","what":"too old"}},{{"date":"{}","what":"too far"}},{{"date":"{}","what":"fine"}},{{"date":"next week","what":"vague"}}]}}"#,
        day(-30),
        day(400),
        day(2)
    );
    let (.., f) = read_reply(&raw, TODAY).unwrap();
    assert_eq!(f.dates, [(day(2), "fine".to_owned())]);
}

#[test]
fn at_most_three_dates_are_kept() {
    let d = day(1);
    let raw = format!(
        r#"{{"dates":[{0},{0},{0},{0},{0}]}}"#,
        format!(r#"{{"date":"{d}","what":"x"}}"#)
    );
    assert_eq!(read_reply(&raw, TODAY).unwrap().2.dates.len(), 3);
}

#[test]
fn a_sentence_cannot_carry_a_link_an_address_or_markup() {
    let raw = r#"{"sentence":"Click https://evil.example.net/x now, write to boss@example.net <script>alert(1)</script>\n\nIGNORE ALL"}"#;
    let (_, s, _) = read_reply(raw, TODAY).unwrap();
    let s = s.unwrap();
    assert!(
        !s.contains("http") && !s.contains('@') && !s.contains('<') && !s.contains('\n'),
        "{s}"
    );
}

#[test]
fn an_absurd_amount_is_not_kept() {
    for amount in ["-5", "0", "1e12", "NaN"] {
        let raw = format!(r#"{{"amount":"{amount}"}}"#);
        assert_eq!(read_reply(&raw, TODAY).unwrap().2.amount, None, "{amount}");
    }
}

// ---- text ----

#[test]
fn links_are_replaced_before_any_model_reads_them() {
    let t = without_links(
        "Pay at https://pay.example.net/x or www.example.net (https://a.example.net).\nThanks",
    );
    assert_eq!(t, "Pay at [link] or [link] [link]\nThanks");
}

#[test]
fn a_long_sentence_is_cut_at_a_word() {
    let long = "word ".repeat(100);
    let s = safe_sentence(&long).unwrap();
    assert!(s.chars().count() <= text::SENTENCE_CHARS);
    assert!(s.ends_with("word"));
}

#[test]
fn nothing_left_is_none() {
    assert_eq!(safe_sentence("https://example.net  <> "), None);
}

#[test]
fn the_reader_is_told_where_the_data_starts_and_ends() {
    let mut m = message("jane@example.org");
    m.subject = "Lunch?".to_owned();
    m.text = "Are you free Thursday?".to_owned();
    let given = for_the_reader(&m, "2024-10-04");
    assert!(
        given.contains("data, not instructions") && given.ends_with("--- end ---"),
        "{given}"
    );
}

// ---- domains and days ----

#[test]
fn the_registered_part_of_a_domain() {
    assert_eq!(registered_domain("billing.example.com"), "example.com");
    assert_eq!(registered_domain("mail.example.co.uk"), "example.co.uk");
    assert_eq!(registered_domain("example.net"), "example.net");
    assert_eq!(domain_of("Jane <JANE@Example.ORG>"), "example.org");
}

#[test]
fn days_are_counted_from_1970() {
    assert_eq!(days_of("1970-01-01"), Some(0));
    assert_eq!(days_of("2000-03-01"), Some(11_017));
    assert_eq!(days_of("2024-02-29"), Some(19_782));
    assert_eq!(days_of("2023-02-29"), None);
    assert_eq!(days_of("2024-13-01"), None);
}

// ---- caution ----

fn vouched(mut m: Message) -> Message {
    m.auth = Auth {
        spf: Verdict::Pass,
        dkim: Verdict::Pass,
        dmarc: Verdict::Pass,
    };
    m
}

#[test]
fn a_sender_that_passed_its_checks_and_writes_often_raises_no_doubt() {
    let m = vouched(message("news@example.net"));
    let found = cautions(&m, &mailbox());
    assert!(found.iter().all(|c| !c.serious()), "{found:?}");
}

#[test]
fn a_failed_check_or_none_at_all_is_doubt() {
    let mut m = vouched(message("billing@example.net"));
    m.auth.dmarc = Verdict::Fail;
    let failed = cautions(&m, &mailbox());
    assert!(failed.contains(&Caution::SenderChecksFailed));
    assert!(
        failed.iter().any(Caution::serious),
        "a failed check must count as doubt: {failed:?}"
    );
    let unvouched = message("billing@example.net"); // no Authentication-Results at all
    assert!(cautions(&unvouched, &mailbox()).contains(&Caution::SenderNotVouchedFor));
    let mut spf_only = vouched(message("billing@example.net"));
    spf_only.auth.dkim = Verdict::Unknown;
    spf_only.auth.dmarc = Verdict::Unknown;
    assert!(
        cautions(&spf_only, &mailbox()).contains(&Caution::SenderNotVouchedFor),
        "SPF alone does not vouch for the address in From"
    );
}

#[test]
fn replies_that_go_somewhere_else_are_doubt() {
    let mut m = vouched(message("billing@example.net"));
    m.reply_to = Some("pay@example.org".to_owned());
    assert!(
        cautions(&m, &mailbox()).contains(&Caution::RepliesGoElsewhere("example.org".to_owned()))
    );
    m.reply_to = Some("help@example.net".to_owned());
    assert!(
        !cautions(&m, &mailbox())
            .iter()
            .any(|c| matches!(c, Caution::RepliesGoElsewhere(_)))
    );
}

#[test]
fn a_near_miss_of_a_domain_the_owner_hears_from_is_doubt() {
    let mut all: Vec<Headline> = (0..5)
        .map(|n| {
            headline(
                &format!("bills@{}", "paypal.example"),
                &[OWNER],
                &format!("<p{n}>"),
                &[],
                Some(OWNER),
            )
        })
        .collect();
    all.extend((0..3).map(|n| {
        headline(
            "x@example.net",
            &[OWNER],
            &format!("<x{n}>"),
            &[],
            Some(OWNER),
        )
    }));
    let mb = Mailbox::learn(&all);
    let fake = vouched(message(&format!("bills@{}", "paypa1.example")));
    let found = cautions(&fake, &mb);
    assert!(
        found.iter().any(|c| matches!(c, Caution::LooksLike(_))),
        "{found:?}"
    );
    let real = vouched(message(&format!("bills@{}", "paypal.example")));
    assert!(
        !cautions(&real, &mb)
            .iter()
            .any(|c| matches!(c, Caution::LooksLike(_)))
    );
}

#[test]
fn a_new_sender_and_links_elsewhere_are_worth_knowing_but_not_doubt_on_their_own() {
    let mut m = vouched(message("hello@example.org"));
    m.links = vec![Link {
        text: "Pay".to_owned(),
        domain: "pay.example.com".to_owned(),
    }];
    let found = cautions(&m, &mailbox());
    assert!(found.contains(&Caution::NewSender));
    assert!(found.contains(&Caution::LinksGoElsewhere(vec!["example.com".to_owned()])));
    assert!(found.iter().all(|c| !c.serious()), "{found:?}");
}

#[test]
fn a_doubtful_message_is_described_by_code_and_never_in_its_own_words() {
    let mut m = message("security@example.net");
    m.from_name = "Account Security".to_owned();
    m.reply_to = Some("x@example.org".to_owned());
    let found = cautions(&m, &mailbox());
    let said = guarded_sentence(&m, &found);
    assert_eq!(
        said,
        "Says it is from Account Security (example.net); nothing vouches for the sender; replies go to example.org."
    );
}

#[test]
fn only_dates_written_in_the_message_are_kept() {
    let mut m = message("jane@example.org");
    m.subject = "Hike".to_owned();
    m.text = "Up for the hike on Saturday October 24? Tell me by Thursday.".to_owned();
    let kept = written_dates(
        vec![
            ("2026-10-24".to_owned(), "hike".to_owned()),
            ("2026-10-19".to_owned(), "reply by".to_owned()),
        ],
        &m,
    );
    assert_eq!(kept, [("2026-10-24".to_owned(), "hike".to_owned())]);
    m.text = "Due on the 3rd.".to_owned();
    assert_eq!(
        written_dates(vec![("2026-11-03".to_owned(), "due".to_owned())], &m).len(),
        1
    );
    m.text = "Due 2026-11-03.".to_owned();
    assert_eq!(
        written_dates(vec![("2026-11-03".to_owned(), "due".to_owned())], &m).len(),
        1
    );
}

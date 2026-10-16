//! The agenda: observations in, items out, each decision a line in Activity.

use super::ports::{Activity, AgendaError, AgendaStore, Done, Item, Pusher, Restarter, Rule};
use crate::domain::{
    self, DAY_MS, EVENT_FYI_LASTS_MS, HOUR_MS, Kind, Level, MINUTE_MS, MOST_PUSHES_A_DAY,
    MOST_WATCHED, RESTART_TAKES_MS, Response, RestartVerdict, Signal, Source,
};

/// How long a closed item is kept.
const KEEP_CLOSED_MS: i64 = 30 * DAY_MS;

/// How the owner answered a pinned item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// It is dealt with.
    Done,
    /// Not worth it.
    Dismiss,
}

/// What an observation changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Change {
    /// Items opened.
    pub opened: usize,
    /// Items whose words changed.
    pub updated: usize,
    /// Items closed because what they were about cleared.
    pub closed: usize,
}

impl Change {
    /// Whether anything changed.
    #[must_use]
    pub const fn any(self) -> bool {
        self.opened + self.updated + self.closed > 0
    }
}

/// The agenda, over its ports.
pub struct Agenda<'a> {
    store: &'a dyn AgendaStore,
    restarter: &'a dyn Restarter,
    pusher: &'a dyn Pusher,
    activity: &'a dyn Activity,
}

impl<'a> Agenda<'a> {
    /// Over its ports.
    pub fn new(
        store: &'a dyn AgendaStore,
        restarter: &'a dyn Restarter,
        pusher: &'a dyn Pusher,
        activity: &'a dyn Activity,
    ) -> Self {
        Self {
            store,
            restarter,
            pusher,
            activity,
        }
    }

    /// Everything a source saw in one full read. A signal not covered by a taught rule opens
    /// an item, or updates the one it already has; for a source of states, an open item it no
    /// longer reports has cleared. Call it only for a read that succeeded.
    ///
    /// # Errors
    /// The store failed.
    pub fn observe(&self, source: Source, signals: &[Signal], now: i64) -> Done<Change> {
        let rules = self.store.rules()?;
        let signals: Vec<&Signal> = signals
            .iter()
            .filter(|s| !rules.iter().any(|r| domain::covered(r.family, &r.about, s)))
            .collect();
        let open: Vec<Item> = self
            .store
            .open_items()?
            .into_iter()
            .filter(|i| i.source == source)
            .collect();
        let mut change = Change::default();
        for signal in &signals {
            let key = signal.key();
            if let Some(item) = open.iter().find(|i| i.key == key) {
                let sentence = signal.sentence();
                if item.sentence != sentence || item.target != signal.target {
                    let mut item = item.clone();
                    item.sentence = sentence;
                    item.target.clone_from(&signal.target);
                    item.updated_at = now;
                    self.store.save_item(&item)?;
                    change.updated += 1;
                }
                continue;
            }
            // Something that happened (mail) is one item for good: answered, it stays answered.
            if !source.reports_states() && self.store.ever_had(&key)? {
                continue;
            }
            self.open(source, signal, now)?;
            change.opened += 1;
        }
        if source.reports_states() {
            for mut item in open {
                if signals.iter().any(|s| s.key() == item.key) {
                    continue;
                }
                let lasted = since(item.opened_at, now);
                self.activity
                    .record(now, &format!("Cleared: {}, after {lasted}.", item.sentence));
                close(&mut item, now, "cleared");
                self.store.save_item(&item)?;
                change.closed += 1;
            }
        }
        self.store.set_checked(source, now)?;
        Ok(change)
    }

    /// What time alone changes: a watch that ran out is told at its level, provided its source
    /// has been read since (a failed read proves nothing); an FYI about something that happened
    /// closes after a day; closed items are kept a month.
    ///
    /// # Errors
    /// The store failed.
    pub fn tick(&self, now: i64) -> Done<bool> {
        let checked = self.store.checked()?;
        let mut changed = false;
        for mut item in self.store.open_items()? {
            if let (Some(until), Some(then)) = (item.watch_until, item.then) {
                let read_since = checked
                    .iter()
                    .any(|(s, at)| *s == item.source && *at >= until);
                if until > now || !read_since {
                    continue;
                }
                item.why = if item.response == Response::Handle {
                    "I restarted it, but it isn't back.".to_owned()
                } else {
                    format!("Still the case after {}.", since(item.opened_at, now))
                };
                item.level = then;
                item.watch_until = None;
                item.then = None;
                item.updated_at = now;
                self.activity.record(
                    now,
                    &format!("{}: {}. {}", then.said(), item.sentence, item.why),
                );
                self.store.save_item(&item)?;
                self.push_if_urgent(&mut item, now)?;
                changed = true;
                continue;
            }
            let quiet = item.level <= Level::Fyi;
            if !item.source.reports_states() && quiet && now - item.opened_at >= EVENT_FYI_LASTS_MS
            {
                close(&mut item, now, "expired");
                self.store.save_item(&item)?;
                changed = true;
            }
        }
        self.store.prune_closed(now - KEEP_CLOSED_MS)?;
        Ok(changed)
    }

    /// The owner answers an item: Done or Dismiss.
    ///
    /// # Errors
    /// No such open item, or the store failed.
    pub fn answer(&self, id: i64, answer: Answer, now: i64) -> Done<()> {
        let mut item = self.open_item(id)?;
        let (how, said) = match answer {
            Answer::Done => ("done", "You marked done"),
            Answer::Dismiss => ("dismissed", "You dismissed"),
        };
        close(&mut item, now, how);
        self.store.save_item(&item)?;
        self.activity
            .record(now, &format!("{said}: {}.", item.sentence));
        Ok(())
    }

    /// Stop: the owner teaches a rule from an item. It closes, with every open item the rule
    /// covers, and the rule holds until undone.
    ///
    /// # Errors
    /// No such open item, or the store failed.
    pub fn stop(&self, id: i64, now: i64) -> Done<Rule> {
        let item = self.open_item(id)?;
        let family = item.kind.family();
        let rule_id = self.store.add_rule(family, &item.about, now)?;
        for mut other in self.store.open_items()? {
            if other.kind.family() == family && other.about.eq_ignore_ascii_case(&item.about) {
                close(&mut other, now, "stopped");
                self.store.save_item(&other)?;
            }
        }
        self.activity.record(
            now,
            &format!("You stopped it: {}.", family.rule_said(&item.about)),
        );
        Ok(Rule {
            id: rule_id,
            family,
            about: item.about,
            at: now,
        })
    }

    /// Undoes a taught rule.
    ///
    /// # Errors
    /// No such rule, or the store failed.
    pub fn forget_rule(&self, id: i64, now: i64) -> Done<()> {
        let rule = self
            .store
            .remove_rule(id)?
            .ok_or_else(|| AgendaError("no such rule".to_owned()))?;
        self.activity.record(
            now,
            &format!("You undid a rule: {}.", rule.family.rule_said(&rule.about)),
        );
        Ok(())
    }

    /// Re-levels a kind, or puts it back to the default with `None`.
    ///
    /// # Errors
    /// The kind cannot be set to that, or the store failed.
    pub fn set_response(&self, kind: Kind, response: Option<Response>, now: i64) -> Done<()> {
        if let Some(r) = response {
            if !kind.allows(r) {
                return Err(AgendaError(format!(
                    "\"{}\" cannot be set to \"{}\"",
                    kind.said(),
                    r.said()
                )));
            }
        }
        self.store.set_response(kind, response)?;
        let to = response.unwrap_or_else(|| kind.default_response());
        self.activity.record(
            now,
            &format!("You set \"{}\" to {}.", kind.said(), to.said()),
        );
        Ok(())
    }

    /// What each kind does now: the owner's re-level, else the default.
    ///
    /// # Errors
    /// The store failed.
    pub fn response_for(&self, kind: Kind) -> Done<Response> {
        Ok(self
            .store
            .responses()?
            .into_iter()
            .find(|(k, r)| *k == kind && kind.allows(*r))
            .map_or_else(|| kind.default_response(), |(_, r)| r))
    }

    fn open_item(&self, id: i64) -> Done<Item> {
        self.store
            .item(id)?
            .filter(|i| i.closed_at.is_none())
            .ok_or_else(|| AgendaError("no such open item".to_owned()))
    }

    /// Opens an item for a new signal: the rules decide, and the decision is a line in Activity.
    fn open(&self, source: Source, signal: &Signal, now: i64) -> Done<()> {
        let mut response = self.response_for(signal.kind)?;
        // Mail that may not be what it says is never more than FYI.
        if signal.cautious && response.level() > Level::Fyi {
            response = Response::Fyi;
        }
        let mut item = Item {
            id: 0,
            key: signal.key(),
            source,
            kind: signal.kind,
            about: signal.about.clone(),
            sentence: signal.sentence(),
            why: String::new(),
            level: response.level(),
            response,
            target: signal.target.clone(),
            opened_at: now,
            updated_at: now,
            watch_until: None,
            then: None,
            pushed: false,
            closed_at: None,
            closed_how: None,
        };
        let label = match response {
            Response::Handle => self.handle(&mut item, signal, now)?,
            Response::LookInto => self.look_into(&mut item, now)?,
            Response::Note => "Noted",
            Response::Fyi | Response::NeedsYou | Response::Urgent => item.level.said(),
        };
        let why = if item.why.is_empty() {
            String::new()
        } else {
            format!(" {}", item.why)
        };
        self.activity
            .record(now, &format!("{label}: {}.{why}", item.sentence));
        item.id = self.store.add_item(&item)?;
        self.push_if_urgent(&mut item, now)
    }

    /// Restarts the container, if the caps allow; otherwise tells.
    fn handle(&self, item: &mut Item, signal: &Signal, now: i64) -> Done<&'static str> {
        let restarts = self.store.restarts_since(now - DAY_MS)?;
        let this_one: Vec<i64> = restarts
            .iter()
            .filter(|(name, _)| *name == signal.about)
            .map(|(_, at)| *at)
            .collect();
        let all: Vec<i64> = restarts.iter().map(|(_, at)| *at).collect();
        let verdict = domain::may_restart(&this_one, &all, now);
        if verdict != RestartVerdict::Restart {
            item.level = if verdict == RestartVerdict::KeepsDying {
                Level::Urgent
            } else {
                Level::NeedsYou
            };
            verdict.said().clone_into(&mut item.why);
            item.why.push('.');
            return Ok(item.level.said());
        }
        let Some(target) = signal.target.as_deref() else {
            item.level = Level::NeedsYou;
            "I had no id to restart it by.".clone_into(&mut item.why);
            return Ok(item.level.said());
        };
        if let Err(why) = self.restarter.restart(target) {
            item.level = Level::NeedsYou;
            item.why = format!("I couldn't restart it: {why}.");
            return Ok(item.level.said());
        }
        self.store.add_restart(&signal.about, now)?;
        "Restarted it.".clone_into(&mut item.why);
        item.watch_until = Some(now + RESTART_TAKES_MS);
        item.then = Some(Level::NeedsYou);
        Ok("Handled")
    }

    /// Watches it with a deadline, unless three things are watched already.
    fn look_into(&self, item: &mut Item, now: i64) -> Done<&'static str> {
        let (lasts, then) = item.kind.watch();
        let watched = self
            .store
            .open_items()?
            .iter()
            .filter(|i| i.response == Response::LookInto && i.watch_until.is_some())
            .count();
        if watched >= MOST_WATCHED {
            item.level = then;
            "I'm already watching three things, so I'm telling you now.".clone_into(&mut item.why);
            return Ok(then.said());
        }
        item.watch_until = Some(now + lasts);
        item.then = Some(then);
        item.why = format!(
            "Watching it; you'll hear if it hasn't cleared in {}.",
            duration(lasts)
        );
        Ok("Watching")
    }

    /// Pushes an urgent item once, at most five a day. About mail it names nothing: the push
    /// leaves the house.
    fn push_if_urgent(&self, item: &mut Item, now: i64) -> Done<()> {
        if item.level != Level::Urgent || item.pushed {
            return Ok(());
        }
        if self.store.pushes_since(now - DAY_MS)? >= MOST_PUSHES_A_DAY {
            self.activity.record(
                now,
                "Didn't push an urgent item: five pushes today already.",
            );
            return Ok(());
        }
        let text = if item.kind.from_mail() {
            "Urgent mail needs a look. It's pinned in Endora.".to_owned()
        } else {
            format!("Urgent: {}.", item.sentence)
        };
        match self.pusher.push(&text) {
            Ok(()) => {
                self.store.add_push(now)?;
                item.pushed = true;
                self.store.save_item(item)
            }
            Err(why) => {
                self.activity
                    .record(now, &format!("Couldn't push an urgent item: {why}."));
                Ok(())
            }
        }
    }
}

fn close(item: &mut Item, now: i64, how: &str) {
    item.closed_at = Some(now);
    item.closed_how = Some(how.to_owned());
    item.watch_until = None;
    item.then = None;
    item.updated_at = now;
}

/// A span, as a person says it: "3 min", "2 h", "1 day".
fn duration(ms: i64) -> String {
    if ms < HOUR_MS {
        return format!("{} min", (ms / MINUTE_MS).max(1));
    }
    if ms < DAY_MS {
        return format!("{} h", ms / HOUR_MS);
    }
    let days = ms / DAY_MS;
    format!("{days} day{}", if days == 1 { "" } else { "s" })
}

fn since(from: i64, now: i64) -> String {
    duration((now - from).max(0))
}

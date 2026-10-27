//! Gaps and broken roads (ADR 0126): the two ways the map fails, kept apart because they are
//! mended differently. A **gap** is knowledge that is missing: a member looked on its map, found
//! nothing and said so; it goes to its lead as one item, never to the owner's desk. A **broken
//! road** is a way that leads nowhere: a section cites an address that does not resolve, found by
//! code each time it is looked for, and shown to the steward of the page that cites it.

use std::ops::ControlFlow;

use super::{Done, Engine};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::map::{self, PageKey};
use crate::domain::world::{Address, Stage};

/// The most hops of a chain of superseded sections followed before giving up.
const HOPS: usize = 5;

/// What reporting a gap came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GapReported {
    /// The gap's number.
    pub id: i64,
    /// Whom it went to, by name.
    pub steward: String,
    /// Whether the same gap was reported before: it is one item, and now has one more report.
    pub already: bool,
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    // ---- gaps ------------------------------------------------------------------------------

    // ---- broken roads ----------------------------------------------------------------------

    /// One step along a section's chain: why it does not resolve (or `None` when it does), or
    /// the address that superseded it, to follow next.
    fn section_hop(&self, page: &str, anchor: &str) -> Done<ControlFlow<Option<String>, String>> {
        let there = PageKey::parse(page)
            .map(|k| self.page_exists(k))
            .transpose()?;
        if there != Some(true) {
            return Ok(ControlFlow::Break(Some("there is no such page".to_owned())));
        }
        let Some(s) = self.section_at(page, anchor)? else {
            return Ok(ControlFlow::Break(Some(
                "nothing is written there".to_owned(),
            )));
        };
        Ok(match (s.stage, s.superseded_by) {
            (Stage::Retired, _) => {
                ControlFlow::Break(Some(format!("the section is retired ({})", s.why)))
            }
            (Stage::Superseded, Some(next)) => ControlFlow::Continue(next),
            (Stage::Superseded, None) => {
                ControlFlow::Break(Some("the section is superseded, naming nothing".to_owned()))
            }
            _ => ControlFlow::Break(None),
        })
    }

    /// Why `address` does not resolve, or `None` when it does: the page or section is there and
    /// current, or it was superseded by something that is.
    fn unresolved(&self, address: &str) -> Done<Option<String>> {
        let mut at = address.to_owned();
        for _ in 0..HOPS {
            let Some(parsed) = Address::parse(&at) else {
                return Ok(Some("it is not an address in the world".to_owned()));
            };
            match parsed {
                Address::Page(slug) => {
                    let there = PageKey::parse(&slug)
                        .map(|k| self.page_exists(k))
                        .transpose()?;
                    return Ok((there != Some(true)).then(|| "there is no such page".to_owned()));
                }
                Address::Section { page, anchor } => match self.section_hop(&page, &anchor)? {
                    ControlFlow::Continue(next) => at = next,
                    ControlFlow::Break(why) => return Ok(why),
                },
            }
        }
        Ok(Some(
            "it leads round in a circle of superseded sections".to_owned(),
        ))
    }

    /// The routes out of a section, for when it is read: each address it cites, and whether it
    /// leads anywhere.
    pub(super) fn routes_said(&self, headline: &str, body: &str, from: &str) -> Done<String> {
        let cited: Vec<String> = map::cited(&format!("{headline} {body}"))
            .into_iter()
            .filter(|c| c != from)
            .collect();
        if cited.is_empty() {
            return Ok(String::new());
        }
        let mut routes = Vec::new();
        for to in cited {
            routes.push(match self.unresolved(&to)? {
                None => format!("{to} (leads there)"),
                Some(why) => format!("{to} (BROKEN: {why})"),
            });
        }
        Ok(format!("Routes out: {}.\n", routes.join("; ")))
    }
}

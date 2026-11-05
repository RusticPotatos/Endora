//! What each model cost (the owner, 2026-10-31): the turns each brain ran, what they spent, what
//! one turn cost and the brain's share of the spend, over the owner's last seven days and the
//! seven before them, so the trend shows. The owner built the first such table by hand from
//! HQ's record for round 2's ten tasks; this is that table, kept.
//!
//! Pure arithmetic over the turns it is handed, each one's brain and spend. Which turns count
//! (those that ran: started, ended, and not a start that never happened) is the scorecard's to
//! say. The house's brains cost nothing and are shown at $0 with their turns. What cannot be
//! known is not guessed: a week with no turns has no cost per turn, and a week with no spend
//! gives no brain a share of it.

use super::brain::Brain;
use super::money::Micros;

/// One brain's turns in one week, or every brain's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Week {
    /// Turns that ran.
    pub turns: usize,
    /// What they spent.
    pub spent: Micros,
}

impl Week {
    /// What one turn cost: the spend over the turns. `None` with no turns, never a cost of
    /// nought and never a division by nothing.
    #[must_use]
    pub fn per_turn(&self) -> Option<Micros> {
        if self.turns == 0 {
            return None;
        }
        Some(self.spent / self.turns as Micros)
    }

    /// This week's part of `all`, the week's spend over every brain, in whole percent rounded
    /// to the nearest. `None` when nothing was spent that week.
    #[must_use]
    pub fn share(&self, all: Micros) -> Option<u8> {
        if all <= 0 {
            return None;
        }
        let part = self.spent.clamp(0, all);
        Some(((part * 100 + all / 2) / all) as u8)
    }

    fn add(&mut self, spent: Micros) {
        self.turns += 1;
        self.spent += spent;
    }
}

/// Which way a brain's spend went from the week before to this one; with the same spend (the
/// house's brains, at $0), which way its turns went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trend {
    /// More than the week before.
    Up,
    /// Less.
    Down,
    /// The same.
    Same,
}

impl Trend {
    /// Its stable name, for the screen.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Same => "same",
        }
    }
}

/// One brain's row: this week and the week before.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    /// The brain the turns ran on.
    pub brain: Brain,
    /// The owner's last seven days.
    pub now: Week,
    /// The seven days before them.
    pub before: Week,
}

impl Row {
    /// Which way it went.
    #[must_use]
    pub fn trend(&self) -> Trend {
        let by_turns = self.now.turns.cmp(&self.before.turns);
        match self.now.spent.cmp(&self.before.spent).then(by_turns) {
            std::cmp::Ordering::Greater => Trend::Up,
            std::cmp::Ordering::Less => Trend::Down,
            std::cmp::Ordering::Equal => Trend::Same,
        }
    }
}

/// The table: a row for each brain that ran a turn in either week, the most spent this week
/// first, and the totals of each week.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ByModel {
    /// One per brain.
    pub rows: Vec<Row>,
    /// Every brain's, this week.
    pub now: Week,
    /// Every brain's, the week before.
    pub before: Week,
}

/// The table from the turns that ran in each week, each as the brain it ran on and what it
/// spent. Rows go by this week's spend, then the week before's, then this week's turns, then
/// the brain's rank, so the order never shifts between two looks at the same record.
#[must_use]
pub fn by_model(now: &[(Brain, Micros)], before: &[(Brain, Micros)]) -> ByModel {
    let mut table = ByModel::default();
    for (this_week, turns) in [(true, now), (false, before)] {
        for &(brain, spent) in turns {
            let at = match table.rows.iter().position(|r| r.brain == brain) {
                Some(at) => at,
                None => {
                    table.rows.push(Row {
                        brain,
                        now: Week::default(),
                        before: Week::default(),
                    });
                    table.rows.len() - 1
                }
            };
            let row = &mut table.rows[at];
            if this_week {
                row.now.add(spent);
                table.now.add(spent);
                continue;
            }
            row.before.add(spent);
            table.before.add(spent);
        }
    }
    table.rows.sort_by(|a, b| {
        b.now
            .spent
            .cmp(&a.now.spent)
            .then(b.before.spent.cmp(&a.before.spent))
            .then(b.now.turns.cmp(&a.now.turns))
            .then(a.brain.rank().cmp(&b.brain.rank()))
    });
    table
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::money::DOLLAR;

    const CENT: Micros = DOLLAR / 100;

    #[test]
    fn a_brain_s_turns_spend_and_cost_a_turn_are_added_up_and_ordered_by_spend() {
        let now = [
            (Brain::Haiku, 2 * CENT),
            (Brain::Sonnet, 40 * CENT),
            (Brain::Haiku, 4 * CENT),
            (Brain::Local, 0),
            (Brain::Local, 0),
            (Brain::Local, 0),
            (Brain::Sonnet, 20 * CENT),
        ];
        let table = by_model(&now, &[]);
        let brains: Vec<Brain> = table.rows.iter().map(|r| r.brain).collect();
        assert_eq!(brains, [Brain::Sonnet, Brain::Haiku, Brain::Local]);
        let sonnet = table.rows[0];
        assert_eq!(
            sonnet.now,
            Week {
                turns: 2,
                spent: 60 * CENT
            }
        );
        assert_eq!(sonnet.now.per_turn(), Some(30 * CENT));
        assert_eq!(
            sonnet.now.share(table.now.spent),
            Some(91),
            "60 of 66 cents"
        );
        assert_eq!(table.rows[1].now.per_turn(), Some(3 * CENT));
        assert_eq!(table.rows[1].now.share(table.now.spent), Some(9));
        // The house's model: its turns counted, at $0 and none of the spend.
        let local = table.rows[2];
        assert_eq!(local.now, Week { turns: 3, spent: 0 });
        assert_eq!(local.now.per_turn(), Some(0));
        assert_eq!(local.now.share(table.now.spent), Some(0));
        assert_eq!(
            table.now,
            Week {
                turns: 7,
                spent: 66 * CENT
            }
        );
    }

    #[test]
    fn an_empty_week_has_no_cost_a_turn_and_no_shares() {
        let table = by_model(&[], &[]);
        assert!(table.rows.is_empty());
        assert_eq!(table.now, Week::default());
        assert_eq!(table.now.per_turn(), None);
        assert_eq!(table.now.share(table.now.spent), None);
        // A week of the house's turns alone: turns, and nothing spent to share.
        let free = by_model(&[(Brain::Mac, 0)], &[]);
        assert_eq!(free.rows[0].now.per_turn(), Some(0));
        assert_eq!(free.rows[0].now.share(free.now.spent), None);
    }

    #[test]
    fn cost_a_turn_with_no_turns_is_none_not_nought() {
        let none = Week { turns: 0, spent: 0 };
        assert_eq!(none.per_turn(), None);
        // A spend with no turns cannot come from the record, and is still no division.
        let odd = Week {
            turns: 0,
            spent: CENT,
        };
        assert_eq!(odd.per_turn(), None);
    }

    #[test]
    fn a_brain_in_only_one_week_has_a_row_with_nothing_in_the_other() {
        let now = [(Brain::Sonnet, 50 * CENT)];
        let before = [(Brain::Haiku, 10 * CENT), (Brain::Haiku, 20 * CENT)];
        let table = by_model(&now, &before);
        let brains: Vec<Brain> = table.rows.iter().map(|r| r.brain).collect();
        assert_eq!(brains, [Brain::Sonnet, Brain::Haiku]);
        let (sonnet, haiku) = (table.rows[0], table.rows[1]);
        assert_eq!(sonnet.before, Week::default());
        assert_eq!(sonnet.before.per_turn(), None);
        assert_eq!(sonnet.before.share(table.before.spent), Some(0));
        assert_eq!(sonnet.trend(), Trend::Up);
        assert_eq!(haiku.now, Week::default());
        assert_eq!(haiku.now.per_turn(), None);
        assert_eq!(haiku.now.share(table.now.spent), Some(0));
        assert_eq!(
            haiku.before,
            Week {
                turns: 2,
                spent: 30 * CENT
            }
        );
        assert_eq!(haiku.before.per_turn(), Some(15 * CENT));
        assert_eq!(haiku.before.share(table.before.spent), Some(100));
        assert_eq!(haiku.trend(), Trend::Down);
    }

    #[test]
    fn the_trend_follows_the_spend_and_for_free_brains_the_turns() {
        let row = |now: Week, before: Week| Row {
            brain: Brain::Local,
            now,
            before,
        };
        let w = |turns, spent| Week { turns, spent };
        assert_eq!(row(w(1, 5), w(9, 4)).trend(), Trend::Up);
        assert_eq!(row(w(9, 4), w(1, 5)).trend(), Trend::Down);
        assert_eq!(row(w(5, 0), w(3, 0)).trend(), Trend::Up);
        assert_eq!(row(w(3, 0), w(5, 0)).trend(), Trend::Down);
        assert_eq!(row(w(3, 7), w(3, 7)).trend(), Trend::Same);
        assert_eq!(Trend::Up.name(), "up");
    }

    #[test]
    fn rows_with_the_same_spend_keep_one_order() {
        let now = [(Brain::Mac, 0), (Brain::Local, 0), (Brain::Local, 0)];
        let before = [(Brain::Mac, 0)];
        let table = by_model(&now, &before);
        let brains: Vec<Brain> = table.rows.iter().map(|r| r.brain).collect();
        // Both at $0: the one with more turns this week first.
        assert_eq!(brains, [Brain::Local, Brain::Mac]);
        let tied = by_model(&[(Brain::Local, 0), (Brain::Mac, 0)], &[]);
        let brains: Vec<Brain> = tied.rows.iter().map(|r| r.brain).collect();
        // All else the same: the abler rank first (the Mac is 5, the house's model 6).
        assert_eq!(brains, [Brain::Mac, Brain::Local]);
    }
}

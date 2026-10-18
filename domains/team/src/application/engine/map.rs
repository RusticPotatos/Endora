//! The team's map (ADR 0126): a page for every area, project, goal and role, each with a steward
//! worked out from the data; the pages made once from the team as it stands and as it grows.
//! Models propose and code decides: who may write a page is a question of the record, never of
//! a name written down.

use serde_json::json;

use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::map::{PageKey, Steward, steward_of};
use crate::domain::world::{self, WorldRefusal};
use crate::domain::{EventKind, GoalStatus, Member, ProjectStatus, Refusal, Role};

fn refused(r: WorldRefusal) -> EngineError {
    EngineError::Refused(Refusal::World(r))
}

/// Every role, for the pages of how each works here.
pub(super) const ROLES: [Role; 5] = [
    Role::Ceo,
    Role::Lead,
    Role::Developer,
    Role::Researcher,
    Role::Reviewer,
];

/// How many pages of each kind a migration made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MapMigrated {
    /// Pages of leads' areas.
    pub areas: usize,
    /// Pages of projects.
    pub projects: usize,
    /// Pages of goals.
    pub goals: usize,
    /// Pages of roles.
    pub roles: usize,
}

impl MapMigrated {
    /// How many pages were made in all.
    #[must_use]
    pub const fn total(&self) -> usize {
        self.areas + self.projects + self.goals + self.roles
    }
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    // ---- the pages ------------------------------------------------------------------------

    /// Makes the pages the team's shape calls for, once each: an area for every lead, a page for
    /// every project and goal that is not given up, and one for every role. Nothing is changed or
    /// deleted, so a second start makes none, and a lead hired later gets its page the next
    /// time. Returns how many of each were made.
    ///
    /// # Errors
    /// A store failure.
    pub fn migrate_map(&self) -> Done<MapMigrated> {
        let _held = self.held();
        self.migrate_map_held()
    }

    /// [`migrate_map`](Self::migrate_map), with the lock held.
    pub(super) fn migrate_map_held(&self) -> Done<MapMigrated> {
        let now = self.clock.now();
        let have = self.store.world_pages()?;
        let make = |key: PageKey| -> Done<usize> {
            let slug = key.slug();
            if have.contains(&slug) {
                return Ok(0);
            }
            Ok(usize::from(self.store.add_world_page(&slug, now)?))
        };
        let mut made = MapMigrated::default();
        for m in self.store.members()? {
            if m.role == Role::Lead {
                made.areas += make(PageKey::Area(m.id))?;
            }
        }
        for p in self.store.projects()? {
            if p.status != ProjectStatus::Dropped {
                made.projects += make(PageKey::Project(p.id))?;
            }
        }
        for g in self.store.goals()? {
            if g.status != GoalStatus::Dropped {
                made.goals += make(PageKey::Goal(g.id))?;
            }
        }
        for role in ROLES {
            made.roles += make(PageKey::Role(role))?;
        }
        if made.total() > 0 {
            self.emit(
                EventKind::WorldChanged,
                json!({ "what": "pages made", "areas": made.areas, "projects": made.projects,
                        "goals": made.goals, "roles": made.roles, "by": "Endora" }),
            )?;
        }
        Ok(made)
    }

    /// Whether the world has a page for `key`: the house and the derived pages always do.
    pub(super) fn page_exists(&self, key: PageKey) -> Done<bool> {
        if matches!(key, PageKey::House | PageKey::Lab | PageKey::Team) {
            return Ok(true);
        }
        Ok(self.store.world_pages()?.contains(&key.slug()))
    }

    /// The page at `slug`, which must be there.
    pub(super) fn page_there(&self, slug: &str) -> Done<PageKey> {
        let key = PageKey::parse(slug)
            .ok_or_else(|| refused(WorldRefusal::NotAnAddress(slug.to_owned())))?;
        if !self.page_exists(key)? {
            return Err(refused(WorldRefusal::NoSuchPage(format!("world:{slug}"))));
        }
        Ok(key)
    }

    /// The page at `slug`, which must be there and written, not derived.
    pub(super) fn written_page(&self, slug: &str) -> Done<PageKey> {
        let key = self.page_there(slug.trim())?;
        if key.derived() {
            return Err(refused(WorldRefusal::Derived(format!(
                "world:{}",
                key.slug()
            ))));
        }
        Ok(key)
    }

    /// Who stewards a page, from the record.
    pub(super) fn steward(&self, key: PageKey) -> Done<Steward> {
        Ok(steward_of(
            key,
            &self.store.members()?,
            &self.store.projects()?,
        ))
    }

    /// A steward by name, for a person to read.
    pub(super) fn steward_called(&self, steward: Steward) -> Done<String> {
        Ok(steward.said(&self.store.members()?))
    }

    /// A page's title, from the data it is about.
    pub(super) fn page_title(&self, key: PageKey) -> Done<String> {
        Ok(match key {
            PageKey::House => world::HOUSE.title.to_owned(),
            PageKey::Lab => world::LAB.title.to_owned(),
            PageKey::Team => world::TEAM.title.to_owned(),
            PageKey::Area(lead) => self
                .store
                .members()?
                .iter()
                .find(|m| m.id == lead)
                .map_or_else(
                    || "A former lead's area".to_owned(),
                    |m| format!("{}'s area", m.name),
                ),
            PageKey::Project(id) => self
                .store
                .projects()?
                .iter()
                .find(|p| p.id == id)
                .map_or_else(
                    || format!("Project {}", id.0),
                    |p| format!("Project: {}", p.title),
                ),
            PageKey::Goal(id) => self.store.goal(id)?.map_or_else(
                || format!("Goal {}", id.0),
                |g| format!("Goal: {}", g.title),
            ),
            PageKey::Role(role) => format!("The {} role", role.title().to_lowercase()),
        })
    }

    /// Whether `who` may write the page directly: its steward may; anyone else proposes.
    pub(super) fn may_write_page(&self, who: &Member, key: PageKey) -> Done<()> {
        let steward = self.steward(key)?;
        if steward.is(who) {
            return Ok(());
        }
        Err(refused(WorldRefusal::ProposeInstead {
            what: self.page_title(key)?,
            steward: self.steward_called(steward)?,
        }))
    }

    /// Whether `who` stewards the page, for confirming and retiring what is on it.
    pub(super) fn may_steward_page(&self, who: &Member, key: PageKey) -> Done<()> {
        let steward = self.steward(key)?;
        if steward.is(who) {
            return Ok(());
        }
        Err(refused(WorldRefusal::NotYours {
            what: self.page_title(key)?,
            steward: self.steward_called(steward)?,
        }))
    }
}

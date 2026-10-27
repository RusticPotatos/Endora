//! The team's map (ADR 0126): a page for every area, project, goal and role, each with a steward
//! worked out from the data; the pages made once from the team as it stands and as it grows.
//! Models propose and code decides: who may write a page is a question of the record, never of
//! a name written down.

use serde_json::json;

use super::{Done, Engine, EngineError};
use crate::application::ports::{Clock, Run, TeamStore, WorkerAdapter};
use crate::domain::map::{self, PageKey, Steward, steward_of};
use crate::domain::situations::Weight;
use crate::domain::world::{self, Section, WorldRefusal};
use crate::domain::{EventKind, GoalStatus, Member, ProjectStatus, Refusal, TaskId};

fn refused(r: WorldRefusal) -> EngineError {
    EngineError::Refused(Refusal::World(r))
}

/// How many pages of each kind a migration made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MapMigrated {
    /// Pages of projects.
    pub projects: usize,
    /// Pages of goals.
    pub goals: usize,
}

impl MapMigrated {
    /// How many pages were made in all.
    #[must_use]
    pub const fn total(&self) -> usize {
        self.projects + self.goals
    }
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    // ---- the pages ------------------------------------------------------------------------

    /// Makes the pages the team's shape calls for, once each: a page for every project and goal
    /// that is not given up. Nothing is changed or
    /// deleted, so a second start makes none, and a lead hired later gets its page the next
    /// time. Returns how many of each were made.
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
        if made.total() > 0 {
            self.emit(
                EventKind::WorldChanged,
                json!({ "what": "pages made", "projects": made.projects, "goals": made.goals,
                        "by": "Endora" }),
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

    /// A page's title, from the data it is about.
    pub(super) fn page_title(&self, key: PageKey) -> Done<String> {
        Ok(match key {
            PageKey::House => world::HOUSE.title.to_owned(),
            PageKey::Lab => world::LAB.title.to_owned(),
            PageKey::Team => world::TEAM.title.to_owned(),
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
        })
    }

    // ---- the map ----------------------------------------------------------------------------

    /// What a page says, as a phrase, from the data it is about.
    fn page_topic(&self, key: PageKey) -> Done<String> {
        Ok(match key {
            PageKey::House => "what the team knows about the house".to_owned(),
            PageKey::Lab => "how the lab is built".to_owned(),
            PageKey::Team => "who is on the team".to_owned(),
            PageKey::Project(id) => {
                let title = self
                    .store
                    .projects()?
                    .iter()
                    .find(|p| p.id == id)
                    .map_or_else(|| id.0.to_string(), |p| p.title.clone());
                format!("what the project \"{title}\" is for, and how it goes")
            }
            PageKey::Goal(id) => {
                let title = self
                    .store
                    .goal(id)?
                    .map_or_else(|| id.0.to_string(), |g| g.title);
                format!("why the goal \"{title}\" exists")
            }
        })
    }

    /// A page as a fact of a map.
    fn page_fact(&self, key: PageKey, weight: Weight) -> Done<map::Fact> {
        Ok(map::Fact::page(
            key,
            &self.page_title(key)?,
            &self.page_topic(key)?,
            steward_of(key).said().to_owned(),
            weight,
        ))
    }

    /// What a member's map is made of: the pages that matter to its task, then the others a line
    /// each, with the best of each page's sections after it.
    pub(super) fn map_facts(&self, task: Option<TaskId>) -> Done<Vec<map::Fact>> {
        let projects = self.store.projects()?;
        let mut facts: Vec<map::Fact> = Vec::new();
        let add = |this: &Self,
                   key: PageKey,
                   weight: Weight,
                   facts: &mut Vec<map::Fact>|
         -> Done<()> {
            if facts.iter().any(|f| !f.section && f.page == key.slug()) || !this.page_exists(key)? {
                return Ok(());
            }
            facts.push(this.page_fact(key, weight)?);
            if key.derived() {
                return Ok(());
            }
            let slug = key.slug();
            let mut sections: Vec<(Section, world::Trust)> = Vec::new();
            let on_page = this.store.sections()?.into_iter();
            for s in on_page.filter(|s| s.page == slug && s.stage.current()) {
                let trust = this.section_trust(&s)?;
                sections.push((s, trust));
            }
            sections.sort_by_key(|(s, t)| (std::cmp::Reverse(t.stage), std::cmp::Reverse(s.id)));
            for (s, trust) in sections.into_iter().take(map::MAP_SECTIONS_PER_PAGE) {
                let weight = match trust.stage {
                    world::Stage::Canon | world::Stage::Insight => Weight::Recommended,
                    _ => Weight::Optional,
                };
                facts.push(map::Fact::section(
                    key,
                    &s.anchor,
                    &s.heading,
                    &s.headline,
                    &this.stage_said(trust),
                    weight,
                ));
            }
            Ok(())
        };
        let task_project = self.project_of(task)?;
        if let Some(project) = task_project {
            add(self, PageKey::Project(project), Weight::Core, &mut facts)?;
        }
        add(self, PageKey::House, Weight::Core, &mut facts)?;
        add(self, PageKey::Lab, Weight::Recommended, &mut facts)?;
        add(self, PageKey::Team, Weight::Recommended, &mut facts)?;
        if let Some(task) = task {
            let goal = self.task(task)?.goal;
            add(self, PageKey::Goal(goal), Weight::Recommended, &mut facts)?;
        }
        // The rest of the projects' pages, a line each, to know of.
        for p in projects
            .iter()
            .filter(|p| map::project_is_current(p.status))
        {
            add(self, PageKey::Project(p.id), Weight::Optional, &mut facts)?;
        }
        Ok(facts)
    }

    /// A member's map, composed within its size: a short one for a turn on the NAS's model, a
    /// longer one for the Mac's large context, the longest for Claude.
    pub(super) fn map_for(&self, member: &Member, run: &Run) -> Done<map::MapText> {
        let cap = map::chars_for(member.brain);
        let entries = self
            .map_facts(run.task)?
            .iter()
            .map(map::Fact::entry)
            .collect();
        let composed = map::compose(entries, cap);
        for address in &composed.kept {
            let Some((page, anchor)) = address
                .strip_prefix("world:")
                .and_then(|s| s.split_once('#'))
            else {
                continue;
            };
            match self.section_at(page, anchor)? {
                Some(section) => self.inherit_input_from(run.id, section.written_in_run)?,
                None => self.unclassified_message_input(run.id)?,
            }
        }
        Ok(composed)
    }

    /// The map as the World card shows it: every page with its steward and what is on it.
    pub fn map_shown(&self) -> Done<serde_json::Value> {
        let sections = self.store.sections()?;
        let mut slugs: Vec<String> = vec![
            PageKey::House.slug(),
            PageKey::Lab.slug(),
            PageKey::Team.slug(),
        ];
        slugs.extend(self.store.world_pages()?);
        let mut pages = Vec::new();
        for slug in slugs {
            let Some(key) = PageKey::parse(&slug) else {
                continue;
            };
            let steward = steward_of(key);
            pages.push(json!({
                "slug": slug, "address": format!("world:{slug}"), "kind": key.kind(),
                "title": self.page_title(key)?, "derived": key.derived(),
                "steward": steward.said(),
                "steward_kind": match steward { Steward::Owner => "owner", Steward::Hq => "hq" },
                "horizon_days": key.horizon_days(),
                "sections": sections.iter().filter(|s| s.page == slug && s.stage.current()).count(),
            }));
        }
        Ok(json!({ "pages": pages }))
    }
}

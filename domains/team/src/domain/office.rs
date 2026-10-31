//! The office as data (ADR 0119, amended by 0142). The open floor is a 32 by 20 grid of tiles;
//! its desks, rooms and walls come from the layout, and its decorations are items: plants,
//! lamps, couches and the like, from one fixed catalog, seeded once. The team no longer changes
//! the floor, so the rules for its proposals went with them.

use super::work::Millis;

/// The floor's width in tiles.
pub const COLS: i64 = 32;
/// The floor's height in tiles, the back wall's two rows included.
pub const ROWS: i64 = 20;
/// The rows at the top that are the back wall.
pub const WALL_ROWS: i64 = 2;

/// What stands in the office.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A potted plant.
    Plant,
    /// A cactus.
    Cactus,
    /// A fern.
    Fern,
    /// A tall palm.
    Palm,
    /// A poster on the wall.
    Poster,
    /// A standing lamp.
    Lamp,
    /// A rug, flat on the floor.
    Rug,
    /// A couch.
    Couch,
    /// A whiteboard on a stand.
    Whiteboard,
    /// A coffee machine.
    CoffeeMachine,
    /// A fish tank.
    FishTank,
    /// A bookshelf.
    Bookshelf,
    /// A bean bag.
    BeanBag,
    /// Fairy lights strung along the wall.
    StringLights,
    /// A clock on the wall.
    Clock,
    /// A framed picture on the wall.
    Art,
}

/// What one kind of item is: its size in tiles, and where it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    /// The kind.
    pub kind: Kind,
    /// Its name, as stored and as members write it.
    pub name: &'static str,
    /// Its width in tiles.
    pub w: i64,
    /// Its height in tiles.
    pub h: i64,
    /// Whether it hangs on the back wall rather than standing on the floor.
    pub wall: bool,
}

const fn spec(kind: Kind, name: &'static str, w: i64, h: i64, wall: bool) -> Spec {
    Spec {
        kind,
        name,
        w,
        h,
        wall,
    }
}

/// The catalog: every kind, and its size and place.
pub const CATALOG: [Spec; 16] = [
    spec(Kind::Plant, "plant", 1, 1, false),
    spec(Kind::Cactus, "cactus", 1, 1, false),
    spec(Kind::Fern, "fern", 1, 1, false),
    spec(Kind::Palm, "palm", 1, 1, false),
    spec(Kind::Poster, "poster", 1, 1, true),
    spec(Kind::Lamp, "lamp", 1, 1, false),
    spec(Kind::Rug, "rug", 2, 2, false),
    spec(Kind::Couch, "couch", 2, 1, false),
    spec(Kind::Whiteboard, "whiteboard", 1, 1, false),
    spec(Kind::CoffeeMachine, "coffee_machine", 1, 1, false),
    spec(Kind::FishTank, "fish_tank", 1, 1, false),
    spec(Kind::Bookshelf, "bookshelf", 2, 1, false),
    spec(Kind::BeanBag, "bean_bag", 1, 1, false),
    spec(Kind::StringLights, "string_lights", 2, 1, true),
    spec(Kind::Clock, "clock", 1, 1, true),
    spec(Kind::Art, "art", 1, 1, true),
];

impl Kind {
    /// The kind's row in the catalog.
    ///
    /// # Panics
    /// Never: the catalog holds every kind (a test checks it).
    #[must_use]
    pub fn spec(self) -> Spec {
        CATALOG
            .iter()
            .copied()
            .find(|s| s.kind == self)
            .expect("the catalog holds every kind")
    }

    /// Its stored name: `fish_tank`.
    #[must_use]
    pub fn name(self) -> &'static str {
        self.spec().name
    }

    /// The kind called this, however it is spelt: `fish_tank`, `Fish Tank`, `fish-tank`.
    #[must_use]
    pub fn from_name(said: &str) -> Option<Self> {
        let said = said.trim().to_lowercase().replace([' ', '-'], "_");
        CATALOG.iter().find(|s| s.name == said).map(|s| s.kind)
    }
}

/// One thing in the office.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// Its number.
    pub id: i64,
    /// What it is.
    pub kind: Kind,
    /// Its left tile.
    pub x: i64,
    /// Its top tile.
    pub y: i64,
    /// Who put it there: a member's name, `endora` or `owner`.
    pub placed_by: String,
    /// When.
    pub placed_at: Millis,
}

/// The office as it has always been laid out, seeded once: the plants, the cactus, the shelf,
/// the lamp and the whiteboard on the commons and the margins, the posters, the clock and the
/// picture on the back wall, and the couches and the coffee machine in the lounge. Ping-pong,
/// the kitchen table and the rooms are the layout's, not items. Each is a kind and its left and
/// top tile.
#[must_use]
pub fn default_layout() -> Vec<(Kind, i64, i64)> {
    let place = |kind, x, y| (kind, x, y);
    vec![
        place(Kind::Plant, 0, 9),
        place(Kind::Cactus, 0, 12),
        place(Kind::Plant, 31, 8),
        place(Kind::Plant, 9, 6),
        place(Kind::Fern, 8, 3),
        place(Kind::Palm, 19, 3),
        place(Kind::Lamp, 11, 2),
        place(Kind::Bookshelf, 13, 2),
        place(Kind::Whiteboard, 16, 2),
        place(Kind::Rug, 11, 4),
        place(Kind::Couch, 17, 18),
        place(Kind::Couch, 20, 18),
        place(Kind::Plant, 22, 18),
        place(Kind::CoffeeMachine, 22, 15),
        place(Kind::BeanBag, 30, 18),
        place(Kind::Art, 21, 1),
        place(Kind::Poster, 25, 1),
        place(Kind::Clock, 28, 1),
        place(Kind::StringLights, 29, 1),
        place(Kind::Poster, 31, 1),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_a_name_that_reads_back() {
        assert_eq!(CATALOG.len(), 16);
        for spec in &CATALOG {
            assert_eq!(Kind::from_name(spec.kind.name()), Some(spec.kind));
        }
        assert_eq!(Kind::from_name("Fish Tank"), Some(Kind::FishTank));
        assert_eq!(Kind::from_name("fish-tank"), Some(Kind::FishTank));
        assert_eq!(Kind::from_name("trampoline"), None);
    }

    #[test]
    fn the_default_layout_stays_on_the_floor_and_wall_things_hang_on_the_wall() {
        let layout = default_layout();
        assert_eq!(layout.len(), 20);
        for (kind, x, y) in layout {
            let spec = kind.spec();
            assert!(
                x >= 0 && x + spec.w <= COLS && y >= 0 && y + spec.h <= ROWS,
                "{kind:?}"
            );
            assert_eq!(spec.wall, y < WALL_ROWS, "{kind:?}");
        }
    }
}

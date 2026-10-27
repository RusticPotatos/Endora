//! The office as data (ADR 0119). The open floor is a 32 by 20 grid of tiles; its desks, rooms
//! and walls come from the layout, and only the things the team may move are items: plants,
//! lamps, couches and the like, from one fixed catalog. Every rule about where an item may
//! stand, and when a proposal to change the office carries, is here as a pure function, so
//! the engine only stores what the rules allow.

use core::fmt;

use serde_json::{Value, json};

use super::work::Millis;

/// The floor's width in tiles.
pub const COLS: i64 = 32;
/// The floor's height in tiles, the back wall's two rows included.
pub const ROWS: i64 = 20;
/// The rows at the top that are the back wall.
pub const WALL_ROWS: i64 = 2;
/// The one row of the wall that wall things hang on.
pub const WALL_ROW: i64 = 1;
/// The most items the floor holds.
pub const MOST_ITEMS: usize = 40;
/// The most changes one proposal makes.
pub const MOST_CHANGES: usize = 5;

/// What the team may put in the office.
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

/// What one kind of item is: its size in tiles, where it goes, and how many the floor holds.
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
    /// The most of this kind the floor holds, if it is limited. The one table of limits.
    pub most: Option<usize>,
}

const fn spec(kind: Kind, name: &'static str, w: i64, h: i64, wall: bool) -> Spec {
    Spec {
        kind,
        name,
        w,
        h,
        wall,
        most: None,
    }
}

/// The catalog: every kind, and its size, place and limit.
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
    Spec {
        most: Some(2),
        ..spec(Kind::FishTank, "fish_tank", 1, 1, false)
    },
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

    /// Its name in words: `fish tank`.
    #[must_use]
    pub fn label(self) -> String {
        self.name().replace('_', " ")
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

/// One change to the office.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// Put a new item on the floor.
    Place {
        /// What.
        kind: Kind,
        /// Its left tile.
        x: i64,
        /// Its top tile.
        y: i64,
    },
    /// Move an item.
    Move {
        /// Which.
        item: i64,
        /// Its new left tile.
        x: i64,
        /// Its new top tile.
        y: i64,
    },
    /// Take an item away.
    Remove {
        /// Which.
        item: i64,
    },
}

/// Why the rules refuse a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// Some of it would stand outside the floor.
    OffTheFloor,
    /// A floor thing set into the back wall.
    OnAWall,
    /// A wall thing off the wall, or over the window, the board, the sign or the painting.
    NotOnTheWall,
    /// In a doorway, or the step in front of it.
    OnADoor,
    /// Among the desks.
    OnADesk,
    /// In a walking lane.
    OnALane,
    /// In a room that is not the team's to decorate, or on its fixed furniture.
    InARoom,
    /// Another item is there.
    Taken(i64),
    /// The floor holds this many already.
    TooMany(usize),
    /// The floor holds this many of the kind already.
    TooManyOf {
        /// The kind.
        kind: Kind,
        /// The most.
        most: usize,
    },
    /// No such item.
    NoSuchItem(i64),
    /// A proposal makes at least one change.
    NoChanges,
    /// A proposal makes at most this many.
    TooManyChanges(usize),
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OffTheFloor => write!(
                f,
                "that is off the floor: it is {COLS} tiles across (x 0 to {}) and {ROWS} down \
                 (y 0 to {})",
                COLS - 1,
                ROWS - 1
            ),
            Self::OnAWall => write!(
                f,
                "the top {WALL_ROWS} rows are the back wall: things that stand go lower down"
            ),
            Self::NotOnTheWall => write!(
                f,
                "wall things (poster, string lights, clock, art) hang on the back wall, y {WALL_ROW}, \
                 where it is free: x 20 to 21 and 25 to 31"
            ),
            Self::OnADoor => f.write_str("that blocks a door, or the step in front of it"),
            Self::OnADesk => f.write_str("that is among the desks: it would be in the way"),
            Self::OnALane => f.write_str("that is in a walking lane people use"),
            Self::InARoom => f.write_str(
                "that is in a room or on fixed furniture, which is not the team's to decorate",
            ),
            Self::Taken(item) => write!(f, "item {item} is already there"),
            Self::TooMany(most) => write!(f, "the floor holds {most} things and has them"),
            Self::TooManyOf { kind, most } => {
                write!(f, "the floor holds at most {most} {}s", kind.label())
            }
            Self::NoSuchItem(item) => write!(f, "there is no item {item}"),
            Self::NoChanges => f.write_str("a proposal changes something"),
            Self::TooManyChanges(most) => write!(f, "a proposal makes at most {most} changes"),
        }
    }
}

/// What stops an item standing on a patch of floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Block {
    Door,
    Room,
    Lane,
    Desk,
}

/// The patches of floor that are not the team's to fill, as `(left, top, right, bottom,
/// why)` with the right and bottom edges excluded; the first that holds is the reason.
const BLOCKED: [(i64, i64, i64, i64, Block); 15] = [
    // The front door, and the step before it.
    (30, 10, 32, 14, Block::Door),
    (28, 9, 30, 15, Block::Door),
    // The rooms, and the fixed furniture of the lounge and the kitchen.
    (0, 2, 7, 7, Block::Room),
    (20, 2, 27, 7, Block::Room),
    (27, 2, 32, 7, Block::Room),
    (9, 15, 16, 20, Block::Room),
    (1, 16, 8, 19, Block::Room),
    (16, 15, 20, 17, Block::Room),
    (24, 16, 29, 19, Block::Room),
    // The games (ADR 0121): the pool table and the row below it where its players stand, and
    // the chess table with a player above it and one below.
    (29, 15, 32, 18, Block::Room),
    (23, 16, 24, 19, Block::Room),
    // The lanes people walk along, under the rooms, above the lower ones, and between them.
    (0, 7, 32, 8, Block::Lane),
    (0, 14, 32, 15, Block::Lane),
    (8, 15, 9, 20, Block::Lane),
    // The band the pods of desks stand in, however many teams there are.
    (1, 8, 28, 14, Block::Desk),
];

/// The back wall that is free to hang things on: the rest is the window, the sign, the board
/// and the CEO's painting. Left edge included, right edge excluded.
const WALL_FREE: [(i64, i64); 2] = [(20, 22), (25, 32)];

/// Where an item stands, in words: a rough place for a proposal to say.
#[must_use]
pub fn area(x: i64, y: i64) -> &'static str {
    let within = |l: i64, t: i64, r: i64, b: i64| x >= l && x < r && y >= t && y < b;
    if y < WALL_ROWS {
        return "the back wall";
    }
    if within(16, 15, 24, 20) {
        return "the lounge";
    }
    if within(24, 15, 32, 20) {
        return "the kitchen";
    }
    if within(0, 15, 9, 20) {
        return "the ping-pong corner";
    }
    if within(9, 15, 16, 20) {
        return "the meeting room side";
    }
    if within(7, 2, 20, 7) {
        return "the commons";
    }
    if within(0, 2, 7, 8) {
        return "the board room side";
    }
    if within(20, 2, 32, 8) {
        return "the CEO's side";
    }
    "the open floor"
}

fn tiles(spec: &Spec, x: i64, y: i64) -> impl Iterator<Item = (i64, i64)> {
    let (w, h) = (spec.w, spec.h);
    (y..y + h).flat_map(move |ty| (x..x + w).map(move |tx| (tx, ty)))
}

/// Whether a place is right for a kind, ignoring the other items.
fn fits(spec: &Spec, x: i64, y: i64) -> Result<(), Refused> {
    if x < 0 || y < 0 || x + spec.w > COLS || y + spec.h > ROWS {
        return Err(Refused::OffTheFloor);
    }
    if spec.wall {
        let free = |tx: i64| WALL_FREE.iter().any(|(l, r)| tx >= *l && tx < *r);
        if y != WALL_ROW || !tiles(spec, x, y).all(|(tx, _)| free(tx)) {
            return Err(Refused::NotOnTheWall);
        }
        return Ok(());
    }
    if y < WALL_ROWS {
        return Err(Refused::OnAWall);
    }
    for (tx, ty) in tiles(spec, x, y) {
        let Some((.., why)) = BLOCKED
            .iter()
            .find(|(l, t, r, b, _)| tx >= *l && tx < *r && ty >= *t && ty < *b)
        else {
            continue;
        };
        return Err(match why {
            Block::Door => Refused::OnADoor,
            Block::Room => Refused::InARoom,
            Block::Lane => Refused::OnALane,
            Block::Desk => Refused::OnADesk,
        });
    }
    Ok(())
}

/// Whether the tiles of a kind at a place overlap another item's, other than `skip`.
fn overlapping(items: &[Item], spec: &Spec, x: i64, y: i64, skip: Option<i64>) -> Option<i64> {
    let mine: Vec<(i64, i64)> = tiles(spec, x, y).collect();
    items
        .iter()
        .filter(|i| Some(i.id) != skip)
        .find(|i| tiles(&i.kind.spec(), i.x, i.y).any(|t| mine.contains(&t)))
        .map(|i| i.id)
}

/// Makes `changes` to `items` one at a time, each judged on what the one before left: so a
/// proposal may take a plant away and put a lamp in its place. Places get the next numbers
/// after the highest there is; who put them and when is for the caller to say. The first
/// change the rules refuse stops it, and nothing is made.
///
/// # Errors
/// What the rules refuse.
pub fn apply(items: &[Item], changes: &[Change]) -> Result<Vec<Item>, Refused> {
    let mut now = items.to_vec();
    let mut next = items.iter().map(|i| i.id).max().unwrap_or(0) + 1;
    // Only what stands now can be moved or taken away: not what this very proposal places.
    let first_new = next;
    for change in changes {
        if let Change::Move { item, .. } | Change::Remove { item } = *change {
            if item >= first_new {
                return Err(Refused::NoSuchItem(item));
            }
        }
        match *change {
            Change::Place { kind, x, y } => {
                let spec = kind.spec();
                fits(&spec, x, y)?;
                if let Some(other) = overlapping(&now, &spec, x, y, None) {
                    return Err(Refused::Taken(other));
                }
                if now.len() >= MOST_ITEMS {
                    return Err(Refused::TooMany(MOST_ITEMS));
                }
                if let Some(most) = spec.most {
                    if now.iter().filter(|i| i.kind == kind).count() >= most {
                        return Err(Refused::TooManyOf { kind, most });
                    }
                }
                now.push(Item {
                    id: next,
                    kind,
                    x,
                    y,
                    placed_by: String::new(),
                    placed_at: 0,
                });
                next += 1;
            }
            Change::Move { item, x, y } => {
                let at = now
                    .iter()
                    .position(|i| i.id == item)
                    .ok_or(Refused::NoSuchItem(item))?;
                let spec = now[at].kind.spec();
                fits(&spec, x, y)?;
                if let Some(other) = overlapping(&now, &spec, x, y, Some(item)) {
                    return Err(Refused::Taken(other));
                }
                now[at].x = x;
                now[at].y = y;
            }
            Change::Remove { item } => {
                let at = now
                    .iter()
                    .position(|i| i.id == item)
                    .ok_or(Refused::NoSuchItem(item))?;
                now.remove(at);
            }
        }
    }
    Ok(now)
}

/// The office as it was before the team changed anything: the plants, the cactus, the shelf,
/// the lamp and the whiteboard on the commons and the margins, the posters, the clock and the
/// picture on the back wall, and the couches and the coffee machine in the lounge. Ping-pong,
/// the kitchen table and the rooms are the layout's, not items.
#[must_use]
pub fn default_layout() -> Vec<Change> {
    let place = |kind, x, y| Change::Place { kind, x, y };
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

impl Change {
    /// A list of changes as stored.
    #[must_use]
    pub fn kept(all: &[Self]) -> String {
        Value::Array(
            all.iter()
                .map(|c| match *c {
                    Self::Place { kind, x, y } => {
                        json!({ "op": "place", "kind": kind.name(), "x": x, "y": y })
                    }
                    Self::Move { item, x, y } => {
                        json!({ "op": "move", "item": item, "x": x, "y": y })
                    }
                    Self::Remove { item } => json!({ "op": "remove", "item": item }),
                })
                .collect(),
        )
        .to_string()
    }

    /// The changes a list as stored holds; nothing for what is not one.
    #[must_use]
    pub fn read(kept: &str) -> Vec<Self> {
        let Ok(Value::Array(all)) = serde_json::from_str::<Value>(kept) else {
            return Vec::new();
        };
        all.iter().filter_map(Self::from_json).collect()
    }

    /// One change from its JSON: `{ op, kind, item, x, y }`.
    #[must_use]
    pub fn from_json(v: &Value) -> Option<Self> {
        let (x, y) = (v["x"].as_i64(), v["y"].as_i64());
        match v["op"].as_str()? {
            "place" => Some(Self::Place {
                kind: Kind::from_name(v["kind"].as_str()?)?,
                x: x?,
                y: y?,
            }),
            "move" => Some(Self::Move {
                item: v["item"].as_i64()?,
                x: x?,
                y: y?,
            }),
            "remove" => Some(Self::Remove {
                item: v["item"].as_i64()?,
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: i64, kind: Kind, x: i64, y: i64) -> Item {
        Item {
            id,
            kind,
            x,
            y,
            placed_by: "endora".to_owned(),
            placed_at: 0,
        }
    }

    fn place(kind: Kind, x: i64, y: i64) -> Change {
        Change::Place { kind, x, y }
    }

    fn refused(items: &[Item], change: Change) -> Refused {
        apply(items, &[change]).expect_err("the rules refuse it")
    }

    #[test]
    fn every_kind_has_a_name_that_reads_back() {
        assert_eq!(CATALOG.len(), 16);
        for spec in &CATALOG {
            assert_eq!(Kind::from_name(spec.kind.name()), Some(spec.kind));
        }
        assert_eq!(Kind::from_name("Fish Tank"), Some(Kind::FishTank));
        assert_eq!(Kind::from_name("fish-tank"), Some(Kind::FishTank));
        assert_eq!(Kind::from_name("trampoline"), None);
        assert_eq!(Kind::FishTank.label(), "fish tank");
    }

    #[test]
    fn a_decoration_goes_on_open_floor() {
        let made = apply(&[], &[place(Kind::Plant, 8, 3)]).expect("a plant in the commons");
        assert_eq!(made.len(), 1);
        assert_eq!((made[0].kind, made[0].x, made[0].y), (Kind::Plant, 8, 3));
    }

    #[test]
    fn nothing_goes_off_the_floor() {
        for (x, y) in [(-1, 5), (32, 5), (5, 20), (5, -1)] {
            assert_eq!(
                refused(&[], place(Kind::Plant, x, y)),
                Refused::OffTheFloor,
                "{x},{y}"
            );
        }
        // A two-tile couch must fit whole.
        assert_eq!(
            refused(&[], place(Kind::Couch, 31, 18)),
            Refused::OffTheFloor
        );
    }

    #[test]
    fn floor_things_are_not_set_into_the_wall_and_wall_things_go_only_on_it() {
        assert_eq!(refused(&[], place(Kind::Plant, 8, 1)), Refused::OnAWall);
        assert_eq!(refused(&[], place(Kind::Plant, 8, 0)), Refused::OnAWall);
        assert_eq!(
            refused(&[], place(Kind::Poster, 28, 5)),
            Refused::NotOnTheWall
        );
        // The window, the sign and the board have the wall from 0 to 20 and the painting
        // the middle of the CEO's: a poster does not cover them.
        assert_eq!(
            refused(&[], place(Kind::Poster, 10, 1)),
            Refused::NotOnTheWall
        );
        assert_eq!(
            refused(&[], place(Kind::Poster, 23, 1)),
            Refused::NotOnTheWall
        );
        assert!(apply(&[], &[place(Kind::Poster, 26, 1)]).is_ok());
        // Strings of lights are two tiles wide: both must be free wall.
        assert_eq!(
            refused(&[], place(Kind::StringLights, 21, 1)),
            Refused::NotOnTheWall
        );
        assert!(apply(&[], &[place(Kind::StringLights, 29, 1)]).is_ok());
    }

    #[test]
    fn nothing_blocks_a_door() {
        // The front door, and the step in front of it.
        for (x, y) in [(30, 10), (31, 13), (28, 11), (29, 9)] {
            assert_eq!(
                refused(&[], place(Kind::Plant, x, y)),
                Refused::OnADoor,
                "{x},{y}"
            );
        }
    }

    #[test]
    fn nothing_goes_among_the_desks_or_in_a_walking_lane() {
        for (x, y) in [(1, 8), (14, 10), (27, 13)] {
            assert_eq!(
                refused(&[], place(Kind::Fern, x, y)),
                Refused::OnADesk,
                "{x},{y}"
            );
        }
        for (x, y) in [(3, 7), (25, 14), (8, 17)] {
            assert_eq!(
                refused(&[], place(Kind::Fern, x, y)),
                Refused::OnALane,
                "{x},{y}"
            );
        }
    }

    #[test]
    fn rooms_are_not_the_teams_to_decorate() {
        for (x, y) in [(3, 4), (22, 3), (29, 4), (12, 17), (4, 17)] {
            assert_eq!(
                refused(&[], place(Kind::Lamp, x, y)),
                Refused::InARoom,
                "{x},{y}"
            );
        }
    }

    #[test]
    fn the_pool_table_and_the_chess_table_are_fixtures_with_their_players_places() {
        // The pool table stands at x 29 to 31, y 15 to 16, its players below it; the chess
        // table at 23,17, its players above and below. None of it is the team's to fill.
        for (x, y) in [
            (29, 15),
            (31, 15),
            (30, 16),
            (29, 17),
            (31, 17),
            (23, 16),
            (23, 17),
            (23, 18),
        ] {
            assert_eq!(
                refused(&[], place(Kind::Lamp, x, y)),
                Refused::InARoom,
                "{x},{y}"
            );
        }
        // What stands around them is still open floor: the coffee machine's place, the plant
        // beside the chess table, the bean bag below the pool table, the kitchen's bin.
        for (x, y) in [(22, 15), (22, 18), (30, 18), (28, 15), (22, 17)] {
            assert!(apply(&[], &[place(Kind::Plant, x, y)]).is_ok(), "{x},{y}");
        }
        // A thing that was already standing there does not stop the rules from judging the rest.
        let rug = [item(1, Kind::Rug, 29, 15)];
        assert!(apply(&rug, &[place(Kind::Plant, 22, 18)]).is_ok());
    }

    #[test]
    fn two_things_do_not_share_a_tile() {
        let items = [item(1, Kind::Couch, 17, 18)];
        assert_eq!(
            refused(&items, place(Kind::Plant, 18, 18)),
            Refused::Taken(1)
        );
        // A rug two tiles square over the couch's end is taken too.
        assert_eq!(refused(&items, place(Kind::Rug, 18, 17)), Refused::Taken(1));
        assert!(apply(&items, &[place(Kind::Plant, 19, 18)]).is_ok());
    }

    #[test]
    fn a_batch_is_judged_a_change_at_a_time() {
        let items = [item(1, Kind::Plant, 8, 3)];
        // Removing the plant frees its tile for the lamp that follows.
        let done = apply(
            &items,
            &[Change::Remove { item: 1 }, place(Kind::Lamp, 8, 3)],
        )
        .expect("the tile is free once the plant is gone");
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].kind, Kind::Lamp);
        // And the reverse order is refused.
        assert_eq!(
            apply(
                &items,
                &[place(Kind::Lamp, 8, 3), Change::Remove { item: 1 }]
            ),
            Err(Refused::Taken(1))
        );
    }

    #[test]
    fn a_proposal_cannot_move_what_it_has_only_just_placed() {
        assert_eq!(
            apply(
                &[],
                &[
                    place(Kind::Plant, 8, 3),
                    Change::Move {
                        item: 1,
                        x: 9,
                        y: 3
                    }
                ]
            ),
            Err(Refused::NoSuchItem(1))
        );
    }

    #[test]
    fn a_move_may_stay_near_where_it_was_and_is_checked_where_it_lands() {
        let items = [item(1, Kind::Couch, 17, 18)];
        // Shifting a couch one tile along overlaps only itself.
        let moved = apply(
            &items,
            &[Change::Move {
                item: 1,
                x: 18,
                y: 18,
            }],
        )
        .expect("it moves");
        assert_eq!((moved[0].x, moved[0].y), (18, 18));
        assert_eq!(
            refused(
                &items,
                Change::Move {
                    item: 1,
                    x: 30,
                    y: 11
                }
            ),
            Refused::OnADoor
        );
        assert_eq!(
            refused(
                &items,
                Change::Move {
                    item: 9,
                    x: 18,
                    y: 18
                }
            ),
            Refused::NoSuchItem(9)
        );
        assert_eq!(
            refused(&items, Change::Remove { item: 9 }),
            Refused::NoSuchItem(9)
        );
    }

    #[test]
    fn the_floor_holds_forty_things_and_two_fish_tanks() {
        let mut items: Vec<Item> = (0..40)
            .map(|i| item(i + 1, Kind::Plant, 8 + i % 12, 2 + i / 12))
            .collect();
        assert_eq!(items.len(), MOST_ITEMS);
        assert_eq!(
            refused(&items, place(Kind::Lamp, 0, 9)),
            Refused::TooMany(MOST_ITEMS)
        );
        // Moving is not adding.
        assert!(
            apply(
                &items,
                &[Change::Move {
                    item: 1,
                    x: 0,
                    y: 9
                }]
            )
            .is_ok()
        );
        items.truncate(30);
        let tanks = [
            item(100, Kind::FishTank, 0, 9),
            item(101, Kind::FishTank, 0, 10),
        ];
        items.extend(tanks);
        assert_eq!(
            refused(&items, place(Kind::FishTank, 0, 11)),
            Refused::TooManyOf {
                kind: Kind::FishTank,
                most: 2
            }
        );
        assert!(apply(&items, &[place(Kind::Fern, 0, 11)]).is_ok());
    }

    #[test]
    fn one_table_holds_every_limit() {
        assert_eq!(Kind::FishTank.spec().most, Some(2));
        let limited: Vec<_> = CATALOG.iter().filter(|s| s.most.is_some()).collect();
        assert_eq!(limited.len(), 1, "only the fish tank is limited");
    }

    #[test]
    fn the_default_layout_is_a_valid_floor_with_every_old_decoration() {
        let seeded = apply(&[], &default_layout()).expect("the seeded layout obeys the rules");
        assert!(seeded.len() <= MOST_ITEMS);
        for kind in [
            Kind::Plant,
            Kind::Cactus,
            Kind::Palm,
            Kind::Bookshelf,
            Kind::Lamp,
            Kind::Poster,
            Kind::Whiteboard,
            Kind::Couch,
            Kind::CoffeeMachine,
            Kind::Clock,
        ] {
            assert!(
                seeded.iter().any(|i| i.kind == kind),
                "{} is not in the default layout",
                kind.name()
            );
        }
    }

    #[test]
    fn a_change_reads_back_from_what_was_stored() {
        let changes = [
            place(Kind::FishTank, 10, 3),
            Change::Move {
                item: 4,
                x: 17,
                y: 18,
            },
            Change::Remove { item: 2 },
        ];
        let kept = Change::kept(&changes);
        assert_eq!(Change::read(&kept), changes);
        assert!(Change::read("not json").is_empty());
    }
}

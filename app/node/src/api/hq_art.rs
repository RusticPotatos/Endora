//! The office's art (ADR 0102), served by name from a fixed list: Pixel Agents (MIT) and
//! MetroCity by JIK-A-4 (CC0). Its credits and licence are in the list too, and travel with
//! every copy (`web/hq-art/CREDITS.txt`). Only these files exist at `/hq-art/`; any other name
//! is not found.

use axum::extract::Path;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;

/// Every file, with its bytes and type.
const ART: &[(&str, &[u8], &str)] = &[
    (
        "CREDITS.txt",
        include_bytes!("../web/hq-art/CREDITS.txt"),
        "text/plain; charset=utf-8",
    ),
    (
        "LICENSE-pixel-agents.txt",
        include_bytes!("../web/hq-art/LICENSE-pixel-agents.txt"),
        "text/plain; charset=utf-8",
    ),
    (
        "characters/char_0.png",
        include_bytes!("../web/hq-art/characters/char_0.png"),
        "image/png",
    ),
    (
        "characters/char_1.png",
        include_bytes!("../web/hq-art/characters/char_1.png"),
        "image/png",
    ),
    (
        "characters/char_2.png",
        include_bytes!("../web/hq-art/characters/char_2.png"),
        "image/png",
    ),
    (
        "characters/char_3.png",
        include_bytes!("../web/hq-art/characters/char_3.png"),
        "image/png",
    ),
    (
        "characters/char_4.png",
        include_bytes!("../web/hq-art/characters/char_4.png"),
        "image/png",
    ),
    (
        "characters/char_5.png",
        include_bytes!("../web/hq-art/characters/char_5.png"),
        "image/png",
    ),
    (
        "floors/floor_1.png",
        include_bytes!("../web/hq-art/floors/floor_1.png"),
        "image/png",
    ),
    (
        "floors/floor_3.png",
        include_bytes!("../web/hq-art/floors/floor_3.png"),
        "image/png",
    ),
    (
        "floors/floor_5.png",
        include_bytes!("../web/hq-art/floors/floor_5.png"),
        "image/png",
    ),
    (
        "floors/floor_7.png",
        include_bytes!("../web/hq-art/floors/floor_7.png"),
        "image/png",
    ),
    (
        "furniture/BIN.png",
        include_bytes!("../web/hq-art/furniture/BIN.png"),
        "image/png",
    ),
    (
        "furniture/BOOKSHELF.png",
        include_bytes!("../web/hq-art/furniture/BOOKSHELF.png"),
        "image/png",
    ),
    (
        "furniture/CACTUS.png",
        include_bytes!("../web/hq-art/furniture/CACTUS.png"),
        "image/png",
    ),
    (
        "furniture/COFFEE.png",
        include_bytes!("../web/hq-art/furniture/COFFEE.png"),
        "image/png",
    ),
    (
        "furniture/CUSHIONED_CHAIR_BACK.png",
        include_bytes!("../web/hq-art/furniture/CUSHIONED_CHAIR_BACK.png"),
        "image/png",
    ),
    (
        "furniture/DESK_FRONT.png",
        include_bytes!("../web/hq-art/furniture/DESK_FRONT.png"),
        "image/png",
    ),
    (
        "furniture/HANGING_PLANT.png",
        include_bytes!("../web/hq-art/furniture/HANGING_PLANT.png"),
        "image/png",
    ),
    (
        "furniture/LARGE_PAINTING.png",
        include_bytes!("../web/hq-art/furniture/LARGE_PAINTING.png"),
        "image/png",
    ),
    (
        "furniture/LARGE_PLANT.png",
        include_bytes!("../web/hq-art/furniture/LARGE_PLANT.png"),
        "image/png",
    ),
    (
        "furniture/PC_FRONT_OFF.png",
        include_bytes!("../web/hq-art/furniture/PC_FRONT_OFF.png"),
        "image/png",
    ),
    (
        "furniture/PC_FRONT_ON_1.png",
        include_bytes!("../web/hq-art/furniture/PC_FRONT_ON_1.png"),
        "image/png",
    ),
    (
        "furniture/PC_FRONT_ON_2.png",
        include_bytes!("../web/hq-art/furniture/PC_FRONT_ON_2.png"),
        "image/png",
    ),
    (
        "furniture/PC_FRONT_ON_3.png",
        include_bytes!("../web/hq-art/furniture/PC_FRONT_ON_3.png"),
        "image/png",
    ),
    (
        "furniture/PLANT.png",
        include_bytes!("../web/hq-art/furniture/PLANT.png"),
        "image/png",
    ),
    (
        "furniture/PLANT_2.png",
        include_bytes!("../web/hq-art/furniture/PLANT_2.png"),
        "image/png",
    ),
    (
        "furniture/SMALL_TABLE_FRONT.png",
        include_bytes!("../web/hq-art/furniture/SMALL_TABLE_FRONT.png"),
        "image/png",
    ),
    (
        "furniture/SOFA_FRONT.png",
        include_bytes!("../web/hq-art/furniture/SOFA_FRONT.png"),
        "image/png",
    ),
];

/// One file of the office's art, by its exact name.
pub(super) async fn hq_art(Path(file): Path<String>) -> impl IntoResponse {
    let Some((_, bytes, kind)) = ART.iter().find(|(name, ..)| *name == file) else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    (
        [
            (header::CONTENT_TYPE, *kind),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        *bytes,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_credits_and_the_licence_travel_with_the_art() {
        let names: Vec<&str> = super::ART.iter().map(|(n, ..)| *n).collect();
        assert!(names.contains(&"CREDITS.txt"));
        assert!(names.contains(&"LICENSE-pixel-agents.txt"));
        assert!(names.iter().filter(|n| n.ends_with(".png")).count() > 20);
    }
}

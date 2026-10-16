//! The brain's use cases (ADR 0101): reading a page for the butler.

use super::*;
use endora_understanding::PagePath;

/// The most of one page a read hands the butler. Pages are notes; one grown past this by
/// hand is cut, with a note, rather than flooding the turn.
const PAGE_IN_A_TURN: usize = 6_000;

/// A page's text for the butler, or, when there is no such page, what pages there are, so a
/// wrong guess at a name is one step from the right one.
///
/// # Errors
/// [`AppError::BadRequest`] for a name that is not a page name;
/// [`AppError::Repository`] if the brain cannot be read.
pub fn read_page(brain: &impl Brain, name: &str) -> Result<String, AppError> {
    let path = PagePath::parse(name).map_err(|e| AppError::BadRequest {
        message: e.to_string(),
    })?;
    if let Some(text) = brain.read(&path)? {
        if text.chars().count() <= PAGE_IN_A_TURN {
            return Ok(text);
        }
        let cut: String = text.chars().take(PAGE_IN_A_TURN).collect();
        return Ok(format!(
            "{cut}\n… (the page goes on; this is as much as a turn carries)"
        ));
    }
    let names: Vec<String> = brain
        .pages()?
        .into_iter()
        .map(|page| page.path.as_str().to_owned())
        .collect();
    if names.is_empty() {
        return Ok(format!("There is no page called {path}, and no pages yet."));
    }
    Ok(format!(
        "There is no page called {path}. The pages are: {}.",
        names.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use endora_understanding::BrainFolder;

    #[test]
    fn a_page_is_read_by_the_name_people_write() {
        let brain = BrainFolder::temporary().unwrap();
        let watches = PagePath::parse("hobbies/watches").unwrap();
        brain
            .change(&watches, "2026-10-16", "test", &mut |_| {
                Some("# Watches\n\nCollects dive watches.\n".to_owned())
            })
            .unwrap();
        assert!(
            read_page(&brain, "Hobbies/Watches")
                .unwrap()
                .contains("Collects dive watches.")
        );
    }

    #[test]
    fn a_wrong_name_gets_the_list_and_a_bad_one_is_refused() {
        let brain = BrainFolder::temporary().unwrap();
        let said = read_page(&brain, "hobbies/watch").unwrap();
        assert!(said.contains("no page called hobbies/watch"), "{said}");
        assert!(said.contains("memory"), "{said}");
        assert!(read_page(&brain, "../etc/passwd").is_err());
    }
}

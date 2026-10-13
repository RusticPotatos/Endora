use super::said_proportionately;

#[test]
fn few_are_named_and_many_are_shaped() {
    let few = vec!["a".to_owned(), "b".to_owned()];
    assert_eq!(said_proportionately(few, "on"), "a; b");
    let many: Vec<String> = (1..=7).map(|i| format!("thing {i}")).collect();
    let said = said_proportionately(many, "on");
    // The shape first, the first few, and the count of the rest — never the recital
    // a small model will read straight back to the person.
    assert!(said.starts_with("7 of them — thing 1;"), "{said}");
    assert!(said.ends_with("and 3 more"), "{said}");
    assert!(!said.contains("thing 5"), "{said}");
}

#[test]
fn nothing_says_so() {
    assert_eq!(
        said_proportionately(Vec::new(), "on the calendar"),
        "Nothing on the calendar just now."
    );
}

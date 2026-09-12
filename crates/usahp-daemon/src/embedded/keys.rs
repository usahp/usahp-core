/// Canonical keyboard names used by embedded capture and persisted mappings.
pub fn normalize(code: &str) -> Option<String> {
    let value = match code {
        "Return" => "Enter",
        "Up" | "UpArrow" => "ArrowUp",
        "Down" | "DownArrow" => "ArrowDown",
        "Left" | "LeftArrow" => "ArrowLeft",
        "Right" | "RightArrow" => "ArrowRight",
        other => other,
    };
    let named = [
        "Space",
        "Enter",
        "Backspace",
        "Tab",
        "Escape",
        "ArrowUp",
        "ArrowDown",
        "ArrowLeft",
        "ArrowRight",
        "Home",
        "End",
        "PageUp",
        "PageDown",
        "Insert",
        "Delete",
    ];
    if named.contains(&value)
        || (value.len() == 1
            && value
                .bytes()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()))
        || value
            .strip_prefix('F')
            .and_then(|n| n.parse::<u8>().ok())
            .is_some_and(|n| (1..=24).contains(&n) && value == format!("F{n}"))
    {
        Some(value.into())
    } else {
        None
    }
}

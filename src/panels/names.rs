//! Names for things: initials for a component's box, and names as file
//! names and URL parts.

/// Up to three initials from a name: "Mach Zehnder switch" -> "MZS".
pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    match words.as_slice() {
        [] => "?".to_owned(),
        [single] => single.chars().take(2).collect::<String>().to_uppercase(),
        many => many.iter().take(3).filter_map(|w| w.chars().next()).collect::<String>().to_uppercase(),
    }
}

/// A name as a file name or URL part: "Mach–Zehnder switch" → "mach-zehnder-switch".
pub fn slug(name: &str) -> String {
    let words: Vec<String> =
        name.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_owned).collect();
    if words.is_empty() { "component".into() } else { words.join("-") }
}

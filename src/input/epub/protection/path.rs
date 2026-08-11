#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum ManifestLocation {
    Archive(String),
    Remote(String),
}

pub(super) fn normalize_manifest_location(
    base: Option<&str>,
    reference: &str,
) -> Option<ManifestLocation> {
    if reference.contains('\\')
        || reference
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return None;
    }
    if has_uri_scheme(reference) {
        return Some(ManifestLocation::Remote(reference.to_owned()));
    }
    normalize_archive_path(base, reference).map(ManifestLocation::Archive)
}

pub(super) fn normalize_archive_path(base: Option<&str>, reference: &str) -> Option<String> {
    let reference_end = reference.find(['?', '#']).unwrap_or(reference.len());
    let decoded = percent_decode(reference.get(..reference_end)?)?;
    if decoded.is_empty()
        || decoded.starts_with('/')
        || decoded.starts_with('\\')
        || decoded.contains('\\')
        || decoded.split('/').next()?.contains(':')
    {
        return None;
    }

    let mut components = Vec::new();
    for component in base
        .into_iter()
        .flat_map(|directory| directory.split('/'))
        .chain(decoded.split('/'))
    {
        match component {
            "" | "." => {}
            ".." => {
                components.pop()?;
            }
            component => components.push(component),
        }
    }
    (!components.is_empty()).then(|| components.join("/"))
}

fn has_uri_scheme(reference: &str) -> bool {
    let Some((scheme, _)) = reference.split_once(':') else {
        return false;
    };
    let mut characters = scheme.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && characters.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        })
}

fn percent_decode(input: &str) -> Option<String> {
    let input = input.as_bytes();
    let mut decoded = Vec::with_capacity(input.len());
    let mut index = 0_usize;
    while index < input.len() {
        if input[index] != b'%' {
            decoded.push(input[index]);
            index += 1;
            continue;
        }
        let high = hex_value(*input.get(index + 1)?)?;
        let low = hex_value(*input.get(index + 2)?)?;
        decoded.push((high << 4) | low);
        index += 3;
    }
    String::from_utf8(decoded).ok()
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

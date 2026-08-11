pub(super) fn is_font_obfuscation_algorithm(algorithm: &str) -> bool {
    matches!(
        algorithm,
        "http://www.idpf.org/2008/embedding" | "http://ns.adobe.com/pdf/enc#RC"
    )
}

pub(super) fn is_font_core_media_type(media_type: &str) -> bool {
    [
        "font/ttf",
        "application/font-sfnt",
        "font/otf",
        "application/vnd.ms-opentype",
        "font/woff",
        "application/font-woff",
        "font/woff2",
    ]
    .iter()
    .any(|candidate| media_type.eq_ignore_ascii_case(candidate))
}

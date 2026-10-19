//! Standard color books (e.g. RAL Classic, Standard/HTML colors).

use super::Rgb;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ColorBookError {
    BookNotFound(String),
    ColorNotFound(String, String),
}

impl std::fmt::Display for ColorBookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ColorBookError::BookNotFound(b) => write!(f, "Cannot find color book \"{b}\"."),
            ColorBookError::ColorNotFound(c, b) => write!(f, "Cannot find color \"{c}\" in color book \"{b}\"."),
        }
    }
}

impl std::error::Error for ColorBookError {}

struct Entry {
    code: &'static str,
    name: &'static str,
    rgb: Rgb,
}

const RAL_ENTRIES: &[Entry] = &[
    Entry { code: "1000", name: "Green beige", rgb: Rgb(206, 198, 140) },
    Entry { code: "1001", name: "Beige", rgb: Rgb(208, 176, 132) },
    Entry { code: "1002", name: "Sand yellow", rgb: Rgb(210, 170, 109) },
    Entry { code: "1003", name: "Signal yellow", rgb: Rgb(237, 164, 4) },
    Entry { code: "1004", name: "Golden yellow", rgb: Rgb(226, 144, 0) },
    Entry { code: "1013", name: "Oyster white", rgb: Rgb(227, 224, 213) },
    Entry { code: "1015", name: "Light ivory", rgb: Rgb(230, 214, 185) },
    Entry { code: "2000", name: "Yellow orange", rgb: Rgb(218, 110, 0) },
    Entry { code: "2004", name: "Pure orange", rgb: Rgb(230, 81, 0) },
    Entry { code: "3000", name: "Flame red", rgb: Rgb(175, 43, 30) },
    Entry { code: "3001", name: "Signal red", rgb: Rgb(160, 33, 40) },
    Entry { code: "3002", name: "Carmine red", rgb: Rgb(155, 35, 33) },
    Entry { code: "3020", name: "Traffic red", rgb: Rgb(204, 6, 5) },
    Entry { code: "4005", name: "Blue lilac", rgb: Rgb(110, 99, 150) },
    Entry { code: "5002", name: "Ultramarine blue", rgb: Rgb(32, 33, 79) },
    Entry { code: "5005", name: "Signal blue", rgb: Rgb(0, 83, 135) },
    Entry { code: "5010", name: "Gentian blue", rgb: Rgb(14, 75, 126) },
    Entry { code: "5012", name: "Light blue", rgb: Rgb(37, 129, 178) },
    Entry { code: "5015", name: "Sky blue", rgb: Rgb(0, 124, 176) },
    Entry { code: "6002", name: "Leaf green", rgb: Rgb(45, 102, 54) },
    Entry { code: "6005", name: "Moss green", rgb: Rgb(47, 69, 56) },
    Entry { code: "6018", name: "Yellow green", rgb: Rgb(87, 166, 57) },
    Entry { code: "7001", name: "Silver grey", rgb: Rgb(140, 150, 157) },
    Entry { code: "7016", name: "Anthracite grey", rgb: Rgb(56, 62, 66) },
    Entry { code: "7024", name: "Graphite grey", rgb: Rgb(71, 74, 81) },
    Entry { code: "7035", name: "Light grey", rgb: Rgb(215, 218, 214) },
    Entry { code: "8017", name: "Chocolate brown", rgb: Rgb(69, 50, 46) },
    Entry { code: "9001", name: "Cream", rgb: Rgb(240, 236, 226) },
    Entry { code: "9005", name: "Jet black", rgb: Rgb(10, 10, 10) },
    Entry { code: "9006", name: "White aluminium", rgb: Rgb(165, 168, 166) },
    Entry { code: "9010", name: "Pure white", rgb: Rgb(247, 249, 245) },
    Entry { code: "9016", name: "Traffic white", rgb: Rgb(246, 246, 246) },
];

const STANDARD_ENTRIES: &[Entry] = &[
    Entry { code: "amber", name: "Amber", rgb: Rgb(255, 191, 0) },
    Entry { code: "aqua", name: "Aqua", rgb: Rgb(0, 255, 255) },
    Entry { code: "azure", name: "Azure", rgb: Rgb(0, 127, 255) },
    Entry { code: "bronze", name: "Bronze", rgb: Rgb(205, 127, 50) },
    Entry { code: "brown", name: "Brown", rgb: Rgb(165, 42, 42) },
    Entry { code: "chartreuse", name: "Chartreuse", rgb: Rgb(127, 255, 0) },
    Entry { code: "coral", name: "Coral", rgb: Rgb(255, 127, 80) },
    Entry { code: "crimson", name: "Crimson", rgb: Rgb(220, 20, 60) },
    Entry { code: "cyan", name: "Cyan", rgb: Rgb(0, 255, 255) },
    Entry { code: "gold", name: "Gold", rgb: Rgb(255, 215, 0) },
    Entry { code: "indigo", name: "Indigo", rgb: Rgb(75, 0, 130) },
    Entry { code: "ivory", name: "Ivory", rgb: Rgb(255, 255, 240) },
    Entry { code: "khaki", name: "Khaki", rgb: Rgb(240, 230, 140) },
    Entry { code: "lavender", name: "Lavender", rgb: Rgb(230, 230, 250) },
    Entry { code: "lime", name: "Lime", rgb: Rgb(0, 255, 0) },
    Entry { code: "maroon", name: "Maroon", rgb: Rgb(128, 0, 0) },
    Entry { code: "navy", name: "Navy", rgb: Rgb(0, 0, 128) },
    Entry { code: "olive", name: "Olive", rgb: Rgb(128, 128, 0) },
    Entry { code: "orange", name: "Orange", rgb: Rgb(255, 165, 0) },
    Entry { code: "pink", name: "Pink", rgb: Rgb(255, 192, 203) },
    Entry { code: "plum", name: "Plum", rgb: Rgb(221, 160, 221) },
    Entry { code: "purple", name: "Purple", rgb: Rgb(128, 0, 128) },
    Entry { code: "red", name: "Red", rgb: Rgb(255, 0, 0) },
    Entry { code: "salmon", name: "Salmon", rgb: Rgb(250, 128, 114) },
    Entry { code: "silver", name: "Silver", rgb: Rgb(192, 192, 192) },
    Entry { code: "teal", name: "Teal", rgb: Rgb(0, 128, 128) },
    Entry { code: "turquoise", name: "Turquoise", rgb: Rgb(64, 224, 208) },
    Entry { code: "violet", name: "Violet", rgb: Rgb(238, 130, 238) },
];

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// Look up a color by book name and color name or code (case-insensitive, ignoring spaces/hyphens).
pub fn lookup_color_book(book: &str, color: &str) -> Result<Rgb, ColorBookError> {
    let b_norm = norm(book);
    let c_norm = norm(color);
    if b_norm.is_empty() {
        return Err(ColorBookError::BookNotFound(book.to_string()));
    }
    let entries = if ["ral", "ralclassic", "raldesign", "raldigital"].iter().any(|k| b_norm == *k) {
        RAL_ENTRIES
    } else if ["standard", "colors", "css", "html", "color"].iter().any(|k| b_norm == *k) {
        STANDARD_ENTRIES
    } else {
        return Err(ColorBookError::BookNotFound(book.to_string()));
    };

    for e in entries {
        if norm(e.code) == c_norm || norm(e.name) == c_norm || format!("ral{}", norm(e.code)) == c_norm {
            return Ok(e.rgb);
        }
    }
    Err(ColorBookError::ColorNotFound(color.to_string(), book.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup_color_book() {
        assert_eq!(lookup_color_book("RAL Classic", "RAL 1000"), Ok(Rgb(206, 198, 140)));
        assert_eq!(lookup_color_book("RAL", "1000"), Ok(Rgb(206, 198, 140)));
        assert_eq!(lookup_color_book("RAL", "1004"), Ok(Rgb(226, 144, 0)));
        assert_eq!(lookup_color_book("ral", "green beige"), Ok(Rgb(206, 198, 140)));
        assert_eq!(lookup_color_book("Standard", "Coral"), Ok(Rgb(255, 127, 80)));
        assert_eq!(lookup_color_book("HTML", "red"), Ok(Rgb(255, 0, 0)));
        assert_eq!(lookup_color_book("General", "1000"), Err(ColorBookError::BookNotFound("General".into())));
        assert!(lookup_color_book("UnknownBook", "Red").is_err());
        assert!(lookup_color_book("RAL", "9999").is_err());
    }
}

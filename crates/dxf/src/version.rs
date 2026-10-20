//! The DXF/DWG file versions CADCraft writes (DXF Reference, HEADER `$ACADVER`).

/// A file version CADCraft can save: the AutoCAD release that introduced the format. Each format
/// also serves the releases after it (2018 is still AutoCAD's current format).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Version {
    /// AutoCAD Release 12 (AC1009), the format of the Italian cadastre's DOCFA and of many
    /// other legacy readers. CADCraft writes it from a reduced copy of the drawing: old-style
    /// POLYLINE/VERTEX, TEXT for multiline text, curves and dimensions as lines; see
    /// `cadcraft_io::dxf_r12`.
    R12,
    /// AutoCAD 2000/2002 (AC1015).
    R2000,
    /// AutoCAD 2004–2006 (AC1018).
    R2004,
    /// AutoCAD 2007–2009 (AC1021): the first version whose DXF text is UTF-8.
    R2007,
    /// AutoCAD 2010–2012 (AC1024).
    R2010,
    /// AutoCAD 2013–2017 (AC1027).
    R2013,
    /// AutoCAD 2018 and later (AC1032), the default.
    #[default]
    R2018,
}

impl Version {
    /// Every version, oldest first.
    pub const ALL: [Version; 7] = [Version::R12, Version::R2000, Version::R2004, Version::R2007, Version::R2010, Version::R2013, Version::R2018];
    /// The versions whose files keep the whole drawing (2000 and later): R12 is a reduced copy.
    pub const LOSSLESS: [Version; 6] = [Version::R2000, Version::R2004, Version::R2007, Version::R2010, Version::R2013, Version::R2018];

    /// The `$ACADVER` code ("AC1032").
    pub fn code(self) -> &'static str {
        match self {
            Version::R12 => "AC1009",
            Version::R2000 => "AC1015",
            Version::R2004 => "AC1018",
            Version::R2007 => "AC1021",
            Version::R2010 => "AC1024",
            Version::R2013 => "AC1027",
            Version::R2018 => "AC1032",
        }
    }

    /// The release year that names the version ("2018"; "R12" for Release 12).
    pub fn year(self) -> &'static str {
        match self {
            Version::R12 => "R12",
            Version::R2000 => "2000",
            Version::R2004 => "2004",
            Version::R2007 => "2007",
            Version::R2010 => "2010",
            Version::R2013 => "2013",
            Version::R2018 => "2018",
        }
    }

    /// A version named by its year ("2018", "R2018"; "12" or "R12" for Release 12) or its
    /// `$ACADVER` code ("AC1032"), any case.
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim();
        let year = s.strip_prefix(['R', 'r']).unwrap_or(s);
        Version::ALL.into_iter().find(|v| v.year().trim_start_matches('R') == year || v.code().eq_ignore_ascii_case(s))
    }

    /// Whether files of this version keep the whole drawing. R12 doesn't: it has no hatches,
    /// multiline text, ellipses, splines, layouts or true colours, so CADCraft writes it from a
    /// reduced copy and the drawing should also be kept in a newer format.
    pub fn lossless(self) -> bool {
        self >= Version::R2000
    }

    /// The version to save a file of `$ACADVER` `code` in: the same version when CADCraft writes
    /// it, else the nearest one it does (R12 for files older than Release 13, R2000 for R13 and
    /// R14 files, 2018 for newer ones); `None` when the code isn't a version code at all.
    pub fn for_acadver(code: &str) -> Option<Version> {
        let code = code.trim();
        if let Some(v) = Version::ALL.into_iter().find(|v| v.code().eq_ignore_ascii_case(code)) {
            return Some(v);
        }
        code.get(..2).filter(|p| p.eq_ignore_ascii_case("AC"))?;
        let n = code.get(2..).filter(|d| d.bytes().all(|b| b.is_ascii_digit())).and_then(|d| d.parse::<u32>().ok())?;
        Some(match n {
            ..1012 => Version::R12,
            1012..1018 => Version::R2000,
            1018..1021 => Version::R2004,
            1021..1024 => Version::R2007,
            1024..1027 => Version::R2010,
            1027..1032 => Version::R2013,
            _ => Version::R2018,
        })
    }

    /// Whether text in this version's DXF is UTF-8 (2007 and later). Older versions keep text in
    /// the `$DWGCODEPAGE` code page, which CADCraft avoids by escaping non-ASCII as `\U+XXXX`.
    pub fn utf8(self) -> bool {
        self >= Version::R2007
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.year())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_years_and_codes() {
        assert_eq!(Version::parse("2018"), Some(Version::R2018));
        assert_eq!(Version::parse(" r2004 "), Some(Version::R2004));
        assert_eq!(Version::parse("ac1021"), Some(Version::R2007));
        assert_eq!(Version::parse("2005"), None);
        assert_eq!(Version::parse(""), None);
        assert_eq!(Version::parse("R12"), Some(Version::R12));
        assert_eq!(Version::parse("12"), Some(Version::R12));
        assert_eq!(Version::parse("ac1009"), Some(Version::R12));
        assert!(!Version::R12.lossless() && Version::R2000.lossless());
        assert!(Version::R12 < Version::R2000 && !Version::R12.utf8());
        for v in Version::ALL {
            assert_eq!(Version::parse(v.code()), Some(v));
            assert_eq!(Version::parse(v.year()), Some(v));
        }
    }

    #[test]
    fn file_versions_map_to_the_nearest_writable_one() {
        assert_eq!(Version::for_acadver("AC1009"), Some(Version::R12));
        assert_eq!(Version::for_acadver("AC1006"), Some(Version::R12));
        assert_eq!(Version::for_acadver("AC1012"), Some(Version::R2000));
        assert_eq!(Version::for_acadver("AC1014"), Some(Version::R2000));
        assert_eq!(Version::for_acadver("AC1024"), Some(Version::R2010));
        assert_eq!(Version::for_acadver("AC1035"), Some(Version::R2018));
        assert_eq!(Version::for_acadver("MC0.0"), None);
        assert_eq!(Version::for_acadver("AC"), None);
        assert_eq!(Version::for_acadver("AC99999999999999999999"), None);
    }
}

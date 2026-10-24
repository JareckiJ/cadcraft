//! An original synthetic DXF after the DXF Reference (DIMENSION, DIMASSOC, DICTIONARY): dimension
//! associativity CADCraft doesn't model. Shared by the I/O and engine tests (by file path).

fn line(h: &str, owner: &str, y: f64) -> String {
    format!("0\nLINE\n5\n{h}\n330\n{owner}\n100\nAcDbEntity\n8\n0\n100\nAcDbLine\n10\n0\n20\n{y}\n30\n0\n11\n10\n21\n{y}\n31\n0\n")
}

/// An aligned dimension along the line at `y`, with its extension dictionary and its reactor to
/// the DIMASSOC.
fn dimension(h: &str, owner: &str, xdict: &str, assoc: &str, y: f64) -> String {
    format!(
        "0\nDIMENSION\n5\n{h}\n102\n{{ACAD_REACTORS\n330\n{assoc}\n102\n}}\n102\n{{ACAD_XDICTIONARY\n360\n{xdict}\n102\n}}\n330\n{owner}\n\
         100\nAcDbEntity\n8\n0\n100\nAcDbDimension\n10\n10\n20\n{d}\n30\n0\n11\n5\n21\n{d}\n31\n0\n70\n33\n3\nStandard\n\
         100\nAcDbAlignedDimension\n13\n0\n23\n{y}\n33\n0\n14\n10\n24\n{y}\n34\n0\n",
        d = y + 2.
    )
}

/// The extension dictionary and the DIMASSOC: the dimension's first point on `geometry` by
/// object snap `osnap` (10 nearest, 8 perpendicular: neither inferred by CADCraft for a line).
fn assoc(xdict: &str, h: &str, dim: &str, geometry: &str, osnap: u8, y: f64) -> String {
    format!(
        "0\nDICTIONARY\n5\n{xdict}\n330\n{dim}\n100\nAcDbDictionary\n280\n1\n281\n1\n3\nACAD_DIMASSOC\n360\n{h}\n\
         0\nDIMASSOC\n5\n{h}\n102\n{{ACAD_REACTORS\n330\n{xdict}\n102\n}}\n330\n{xdict}\n100\nAcDbDimAssoc\n330\n{dim}\n90\n1\n70\n0\n71\n0\n\
         1\nAcDbOsnapPointRef\n72\n{osnap}\n331\n{geometry}\n73\n0\n91\n0\n40\n0.0\n10\n0\n20\n{y}\n30\n0\n75\n0\n"
    )
}

/// A block holding a line and a dimension on it (nearest snap), and a model space line with a
/// dimension (perpendicular snap).
pub fn source() -> String {
    let mut s = String::from("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1015\n9\n$HANDSEED\n5\n100\n0\nENDSEC\n");
    s.push_str("0\nSECTION\n2\nCLASSES\n0\nCLASS\n1\nDIMASSOC\n2\nAcDbDimAssoc\n3\nObjectDBX Classes\n90\n0\n280\n0\n281\n0\n0\nENDSEC\n");
    s.push_str("0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nBLOCK_RECORD\n5\n1\n70\n2\n");
    s.push_str("0\nBLOCK_RECORD\n5\n1F\n330\n1\n100\nAcDbSymbolTableRecord\n100\nAcDbBlockTableRecord\n2\n*Model_Space\n");
    s.push_str("0\nBLOCK_RECORD\n5\n20\n330\n1\n100\nAcDbSymbolTableRecord\n100\nAcDbBlockTableRecord\n2\nWALL\n");
    s.push_str("0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n");
    s.push_str("0\nBLOCK\n5\n21\n330\n20\n100\nAcDbEntity\n8\n0\n100\nAcDbBlockBegin\n2\nWALL\n70\n0\n10\n0\n20\n0\n30\n0\n3\nWALL\n1\n\n");
    s.push_str(&line("30", "20", 0.));
    s.push_str(&dimension("31", "20", "40", "41", 0.));
    s.push_str("0\nENDBLK\n5\n22\n330\n20\n100\nAcDbEntity\n8\n0\n100\nAcDbBlockEnd\n0\nENDSEC\n");
    s.push_str("0\nSECTION\n2\nENTITIES\n");
    s.push_str(&line("50", "1F", 20.));
    s.push_str(&dimension("51", "1F", "60", "61", 20.));
    s.push_str("0\nENDSEC\n0\nSECTION\n2\nOBJECTS\n0\nDICTIONARY\n5\nC\n330\n0\n100\nAcDbDictionary\n281\n1\n");
    s.push_str(&assoc("40", "41", "31", "30", 10, 0.));
    s.push_str(&assoc("60", "61", "51", "50", 8, 20.));
    s.push_str("0\nENDSEC\n0\nEOF\n");
    s
}

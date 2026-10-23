//! A minimal TrueType font for the PDF text tests: .notdef, "A" (triangle), "B" (square).
//! Shared by `tests/pdf_text.rs` and the unit tests of `src/pdf.rs`.

fn be16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_be_bytes());
}
fn be32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_be_bytes());
}

/// A simple glyph: one closed contour of on-curve points.
fn simple_glyph(pts: &[(i16, i16)]) -> Vec<u8> {
    let mut g = Vec::new();
    let xs: Vec<i16> = pts.iter().map(|p| p.0).collect();
    let ys: Vec<i16> = pts.iter().map(|p| p.1).collect();
    be16(&mut g, 1);
    for v in [*xs.iter().min().unwrap(), *ys.iter().min().unwrap(), *xs.iter().max().unwrap(), *ys.iter().max().unwrap()] {
        be16(&mut g, v as u16);
    }
    be16(&mut g, pts.len() as u16 - 1);
    be16(&mut g, 0);
    g.extend(std::iter::repeat_n(1u8, pts.len()));
    let (mut px, mut py) = (0i16, 0i16);
    for (x, _) in pts {
        be16(&mut g, (x - px) as u16);
        px = *x;
    }
    for (_, y) in pts {
        be16(&mut g, (y - py) as u16);
        py = *y;
    }
    while g.len() % 4 != 0 {
        g.push(0);
    }
    g
}

/// A TrueType font with glyphs .notdef (empty), A and B, and OS/2 `fsType`.
pub fn test_font(fs_type: u16) -> Vec<u8> {
    let glyphs = [Vec::new(), simple_glyph(&[(50, 0), (550, 0), (300, 700)]), simple_glyph(&[(50, 0), (550, 0), (550, 700), (50, 700)])];
    let mut glyf = Vec::new();
    let mut loca = Vec::new();
    for g in &glyphs {
        be32(&mut loca, glyf.len() as u32);
        glyf.extend_from_slice(g);
    }
    be32(&mut loca, glyf.len() as u32);
    let mut head = Vec::new();
    be32(&mut head, 0x0001_0000);
    be32(&mut head, 0x0001_0000);
    be32(&mut head, 0);
    be32(&mut head, 0x5F0F_3CF5);
    be16(&mut head, 0);
    be16(&mut head, 1000);
    head.extend_from_slice(&[0; 16]);
    for v in [0i16, 0, 600, 700] {
        be16(&mut head, v as u16);
    }
    for v in [0u16, 8, 2, 1, 0] {
        be16(&mut head, v);
    }
    let mut hhea = Vec::new();
    be32(&mut hhea, 0x0001_0000);
    for v in [800i16, -200, 0, 600, 0, 0, 600, 1, 0, 0, 0, 0, 0, 0, 0] {
        be16(&mut hhea, v as u16);
    }
    be16(&mut hhea, 3);
    let mut maxp = Vec::new();
    be32(&mut maxp, 0x0001_0000);
    for v in [3u16, 4, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0] {
        be16(&mut maxp, v);
    }
    let mut hmtx = Vec::new();
    for _ in 0..3 {
        be16(&mut hmtx, 600);
        be16(&mut hmtx, 50);
    }
    // cmap: format 4, 'A'..'B' → 1..2.
    let mut sub = Vec::new();
    let segs: [(u16, u16, u16); 2] = [(0x41, 0x42, 1u16.wrapping_sub(0x41)), (0xFFFF, 0xFFFF, 1)];
    be16(&mut sub, 4);
    be16(&mut sub, (16 + segs.len() * 8) as u16);
    be16(&mut sub, 0);
    be16(&mut sub, (segs.len() * 2) as u16);
    for v in [4u16, 1, 0] {
        be16(&mut sub, v);
    }
    for s in segs {
        be16(&mut sub, s.1);
    }
    be16(&mut sub, 0);
    for s in segs {
        be16(&mut sub, s.0);
    }
    for s in segs {
        be16(&mut sub, s.2);
    }
    for _ in segs {
        be16(&mut sub, 0);
    }
    let mut cmap = Vec::new();
    for v in [0u16, 1, 3, 1] {
        be16(&mut cmap, v);
    }
    be32(&mut cmap, 12);
    cmap.extend_from_slice(&sub);
    let mut os2 = vec![0u8; 78];
    os2[8..10].copy_from_slice(&fs_type.to_be_bytes());
    let mut tables: Vec<(&[u8; 4], Vec<u8>)> =
        vec![(b"OS/2", os2), (b"cmap", cmap), (b"glyf", glyf), (b"head", head), (b"hhea", hhea), (b"hmtx", hmtx), (b"loca", loca), (b"maxp", maxp)];
    tables.sort_by_key(|t| *t.0);
    let mut out = Vec::new();
    be32(&mut out, 0x0001_0000);
    for v in [tables.len() as u16, 128, 3, tables.len() as u16 * 16 - 128] {
        be16(&mut out, v);
    }
    let mut off = 12 + 16 * tables.len();
    for (t, d) in &tables {
        out.extend_from_slice(*t);
        be32(&mut out, 0);
        be32(&mut out, off as u32);
        be32(&mut out, d.len() as u32);
        off += d.len().div_ceil(4) * 4;
    }
    for (_, d) in &tables {
        out.extend_from_slice(d);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    out
}

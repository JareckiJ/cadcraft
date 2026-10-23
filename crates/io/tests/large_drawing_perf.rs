//! Throughput of opening and saving large drawings (gap FF6, #57: a 10 MB DWG took about two
//! minutes and then failed). A drawing is generated in code, with the mix of a real project
//! (lines, arcs, polylines, text, multiline text, block references with attributes, hatches,
//! dimensions and extended data on many layers), so no large file is kept in the repository.
//!
//! The guard is twofold: a mid-size drawing opens and round-trips through DXF and DWG under a
//! generous time bound, and opening four times the entities takes well under sixteen times as
//! long, which catches work that grows with the square of the drawing.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

/// A DXF drawing of about `n` model-space entities, `n / 1000` (at least 10) blocks of a dozen
/// entities and an attribute definition each, and 64 layers.
pub fn generate(n: usize) -> String {
    generate_with(n, (n / 1000).max(10), 12)
}

/// [`generate`] with `blocks` blocks (at least one) of `per_block` entities and an attribute
/// definition each.
pub fn generate_with(n: usize, blocks: usize, per_block: u32) -> String {
    let blocks = blocks.max(1);
    let layers = 64;
    let mut s = String::with_capacity(n.saturating_mul(260));
    let mut g = |code: i32, v: &dyn std::fmt::Display| {
        let _ = write!(s, "{code:>3}\r\n{v}\r\n");
    };
    let sec = |g: &mut dyn FnMut(i32, &dyn std::fmt::Display), name: &str| {
        g(0, &"SECTION");
        g(2, &name);
    };
    sec(&mut g, "HEADER");
    g(9, &"$ACADVER");
    g(1, &"AC1032");
    g(9, &"$INSUNITS");
    g(70, &4);
    g(9, &"$HANDSEED");
    g(5, &format!("{:X}", 0x1000 + n * 4));
    g(0, &"ENDSEC");
    sec(&mut g, "TABLES");
    g(0, &"TABLE");
    g(2, &"LAYER");
    for l in 0..layers {
        g(0, &"LAYER");
        g(2, &format!("LAYER-{l:02}"));
        g(70, &0);
        g(62, &(1 + l % 250));
        g(6, &"Continuous");
    }
    g(0, &"ENDTAB");
    g(0, &"TABLE");
    g(2, &"APPID");
    g(0, &"APPID");
    g(2, &"CADCRAFT_PERF");
    g(70, &0);
    g(0, &"ENDTAB");
    g(0, &"ENDSEC");
    sec(&mut g, "BLOCKS");
    for b in 0..blocks {
        g(0, &"BLOCK");
        g(8, &"0");
        g(2, &format!("PART-{b}"));
        g(70, &2);
        g(10, &0.0);
        g(20, &0.0);
        g(30, &0.0);
        for k in 0..per_block {
            let (x, y) = (f64::from(k) * 1.5, f64::from(k % 3) * 2.0);
            match k % 4 {
                0 => {
                    g(0, &"LINE");
                    g(8, &"0");
                    g(10, &x);
                    g(20, &y);
                    g(11, &(x + 1.25));
                    g(21, &(y + 0.75));
                }
                1 => {
                    g(0, &"CIRCLE");
                    g(8, &"0");
                    g(10, &x);
                    g(20, &y);
                    g(40, &0.5);
                }
                2 => {
                    g(0, &"ARC");
                    g(8, &"0");
                    g(10, &x);
                    g(20, &y);
                    g(40, &0.75);
                    g(50, &15.0);
                    g(51, &195.0);
                }
                _ => {
                    g(0, &"TEXT");
                    g(8, &"0");
                    g(10, &x);
                    g(20, &y);
                    g(40, &0.25);
                    g(1, &format!("P{b}.{k}"));
                }
            }
        }
        g(0, &"ATTDEF");
        g(8, &"0");
        g(10, &0.0);
        g(20, &-1.0);
        g(40, &0.3);
        g(1, &"?");
        g(3, &"Part number");
        g(2, &"PARTNO");
        g(70, &0);
        g(0, &"ENDBLK");
        g(8, &"0");
    }
    g(0, &"ENDSEC");
    sec(&mut g, "ENTITIES");
    // Entities carry handles, as every file of R13 and later does (extended data is kept by them).
    let mut next_handle = 0x1000u64;
    let side = ((n as f64).sqrt() * 10.0).max(10.0);
    let row = (n as f64).sqrt().ceil().max(1.0) as usize;
    for i in 0..n {
        let x = (i % row) as f64 * side / row as f64;
        let y = (i / row) as f64 * side / row as f64;
        let layer = format!("LAYER-{:02}", i % layers);
        let mut handle = || {
            next_handle += 1;
            format!("{next_handle:X}")
        };
        match i % 10 {
            0 => {
                g(0, &"LINE");
                g(5, &handle());
                g(8, &layer);
                g(10, &x);
                g(20, &y);
                g(11, &(x + 7.5));
                g(21, &(y + 3.25));
                g(1001, &"CADCRAFT_PERF");
                g(1000, &format!("item {i}"));
                g(1040, &(i as f64 * 0.5));
                g(1070, &((i % 30_000) as i64));
            }
            1 => {
                g(0, &"CIRCLE");
                g(5, &handle());
                g(8, &layer);
                g(10, &x);
                g(20, &y);
                g(40, &(1.0 + (i % 7) as f64));
            }
            2 => {
                g(0, &"ARC");
                g(5, &handle());
                g(8, &layer);
                g(10, &x);
                g(20, &y);
                g(40, &2.5);
                g(50, &((i % 360) as f64));
                g(51, &(((i + 120) % 360) as f64));
            }
            3 => {
                g(0, &"LWPOLYLINE");
                g(5, &handle());
                g(8, &layer);
                g(90, &8);
                g(70, &(i % 2));
                for k in 0..8 {
                    g(10, &(x + f64::from(k)));
                    g(20, &(y + f64::from(k % 3) * 0.75));
                    if k == 3 {
                        g(42, &0.4);
                    }
                }
            }
            4 => {
                g(0, &"TEXT");
                g(5, &handle());
                g(8, &layer);
                g(10, &x);
                g(20, &y);
                g(40, &1.5);
                g(1, &format!("Label {i}"));
            }
            5 => {
                g(0, &"MTEXT");
                g(5, &handle());
                g(8, &layer);
                g(10, &x);
                g(20, &y);
                g(40, &1.25);
                g(41, &30.0);
                g(71, &1);
                g(1, &format!("{{\\fArial|b1;Room {i}}}\\PArea: {} m²", i % 97));
            }
            6 => {
                g(0, &"INSERT");
                g(5, &handle());
                g(8, &layer);
                g(66, &1);
                g(2, &format!("PART-{}", i % blocks));
                g(10, &x);
                g(20, &y);
                g(50, &((i % 4) as f64 * 90.0));
                g(0, &"ATTRIB");
                g(5, &handle());
                g(8, &layer);
                g(10, &x);
                g(20, &(y - 1.0));
                g(40, &0.3);
                g(1, &format!("PN-{i:07}"));
                g(2, &"PARTNO");
                g(70, &0);
                g(0, &"SEQEND");
                g(5, &handle());
                g(8, &layer);
            }
            7 => {
                let solid = i % 20 == 7;
                g(0, &"HATCH");
                g(5, &handle());
                g(8, &layer);
                g(10, &0.0);
                g(20, &0.0);
                g(30, &0.0);
                g(210, &0.0);
                g(220, &0.0);
                g(230, &1.0);
                g(2, &if solid { "SOLID" } else { "ANSI31" });
                g(70, &i32::from(solid));
                g(71, &0);
                g(91, &1);
                g(92, &3);
                g(72, &0);
                g(73, &1);
                g(93, &4);
                for (dx, dy) in [(0.0, 0.0), (6.0, 0.0), (6.0, 4.0), (0.0, 4.0)] {
                    g(10, &(x + dx));
                    g(20, &(y + dy));
                }
                g(97, &0);
                g(75, &0);
                g(76, &1);
                if !solid {
                    g(52, &0.0);
                    g(41, &1.0);
                    g(77, &0);
                    g(78, &1);
                    g(53, &45.0);
                    g(43, &0.0);
                    g(44, &0.0);
                    g(45, &-2.245_064_4);
                    g(46, &2.245_064_4);
                    g(79, &0);
                }
                g(98, &0);
            }
            8 => {
                g(0, &"DIMENSION");
                g(5, &handle());
                g(8, &layer);
                g(10, &(x + 8.0));
                g(20, &(y + 3.0));
                g(11, &(x + 4.0));
                g(21, &(y + 3.0));
                g(70, &32);
                g(3, &"Standard");
                g(13, &x);
                g(23, &y);
                g(14, &(x + 8.0));
                g(24, &y);
                g(50, &0.0);
            }
            _ => {
                g(0, &"INSERT");
                g(5, &handle());
                g(8, &layer);
                g(2, &format!("PART-{}", (i / 10) % blocks));
                g(10, &x);
                g(20, &y);
                g(41, &1.5);
                g(42, &1.5);
            }
        }
    }
    g(0, &"ENDSEC");
    g(0, &"EOF");
    s
}

/// Blocks other than anonymous ones (the dimensions' blocks a save adds).
fn named_blocks(d: &cadcraft_doc::Drawing) -> usize {
    d.blocks.keys().filter(|k| !k.starts_with('*')).count()
}

/// The shortest of `runs` timings of `f` (the least disturbed by other work on the machine).
fn fastest<T>(runs: usize, mut f: impl FnMut() -> T) -> (Duration, T) {
    let mut best: Option<(Duration, T)> = None;
    for _ in 0..runs.max(1) {
        let t = Instant::now();
        let v = f();
        let e = t.elapsed();
        if best.as_ref().is_none_or(|(b, _)| e < *b) {
            best = Some((e, v));
        }
    }
    match best {
        Some(b) => b,
        None => (Duration::ZERO, f()),
    }
}

#[test]
fn generated_drawing_holds_what_was_asked() {
    let d = cadcraft_io::read_dxf(generate(1000).as_bytes()).expect("read");
    assert_eq!(d.model.len(), 1000);
    assert_eq!(named_blocks(&d), 10);
    use cadcraft_doc::EntityKind as K;
    let count = |f: fn(&K) -> bool| d.model.iter().filter(|e| f(&e.kind)).count();
    assert_eq!(count(|k| matches!(k, K::Hatch(_))), 100);
    assert_eq!(count(|k| matches!(k, K::Dimension(_))), 100);
    assert_eq!(count(|k| matches!(k, K::Insert(i) if !i.attribs.is_empty())), 100);
    assert_eq!(d.model.iter().filter(|e| !e.common.xdata.is_empty()).count(), 100);
}

/// A mid-size drawing (20,000 entities, about 5 MB of DXF and 1 MB of DWG) opens, saves and
/// reopens as DXF and DWG within a bound far above what it takes (a second or two each), so
/// only a real slowdown trips it, also on a slow CI runner.
#[test]
fn mid_size_drawing_opens_and_round_trips_in_time() {
    const N: usize = 20_000;
    const BOUND: Duration = Duration::from_secs(60);
    let text = generate(N);
    let t = Instant::now();
    let d = cadcraft_io::read_dxf(text.as_bytes()).expect("read the generated DXF");
    let read = t.elapsed();
    assert_eq!(d.model.len(), N);

    let t = Instant::now();
    let dxf = cadcraft_io::write(&d, "big.dxf").expect("save DXF");
    let back = cadcraft_io::read(&dxf, "big.dxf").expect("reopen DXF");
    let dxf_trip = t.elapsed();
    assert_eq!(back.model.len(), N);

    let t = Instant::now();
    let dwg = cadcraft_io::write(&d, "big.dwg").expect("save DWG");
    let save_dwg = t.elapsed();
    let t = Instant::now();
    let back = cadcraft_io::read(&dwg, "big.dwg").expect("reopen DWG");
    let open_dwg = t.elapsed();
    assert_eq!(back.model.len(), N);
    assert_eq!(named_blocks(&back), named_blocks(&d));

    eprintln!(
        "{N} entities: read {read:?}, DXF save+reopen {dxf_trip:?} ({} KB), DWG save {save_dwg:?} ({} KB), DWG open {open_dwg:?}",
        dxf.len() / 1024,
        dwg.len() / 1024
    );
    for (what, took) in [("DXF read", read), ("DXF save and reopen", dxf_trip), ("DWG save", save_dwg), ("DWG open", open_dwg)] {
        assert!(took < BOUND, "{what} of {N} entities took {took:?}, more than {BOUND:?}");
    }
}

/// Opening and saving four times the entities takes well under sixteen times as long (linear
/// work takes about four), for DXF and DWG: the guard against a lookup or copy that grows with
/// the square of the drawing. The drawing has a small block for every two entities (and every
/// dimension gets one when saved), so work done per block for every entity or record shows
/// (#57: saving looked up each block's end among all records). Each step is timed three times
/// and the fastest kept, and the small size is big enough (tens of milliseconds) that timer noise
/// doesn't decide.
#[test]
fn opening_and_saving_scale_linearly() {
    const SMALL: usize = 3_000;
    const FACTOR: usize = 4;
    // Linear is 4x; quadratic would be 16x. A loaded runner adds noise both ways.
    const MAX_RATIO: f64 = 9.0;
    let read = |text: &str| cadcraft_io::read_dxf(text.as_bytes()).expect("read the generated DXF");
    let (small, large) = (read(&generate_with(SMALL, SMALL / 2, 2)), read(&generate_with(SMALL * FACTOR, SMALL * FACTOR / 2, 2)));
    assert_eq!(large.model.len(), SMALL * FACTOR);
    let save = |d: &cadcraft_doc::Drawing, name: &str| cadcraft_io::write(d, name).expect("save");
    let (dxf_small, dxf_large) = (save(&small, "x.dxf"), save(&large, "x.dxf"));
    let (dwg_small, dwg_large) = (save(&small, "x.dwg"), save(&large, "x.dwg"));
    let open = |bytes: &[u8], name: &str| cadcraft_io::read(bytes, name).expect("open");
    let count = |bytes: &[u8], name: &str| open(bytes, name).model.len();
    // Saved again as read back from its own file, as a drawing opened and saved is: it keeps the
    // file's handles and other data, which saving puts back.
    let (again_small, again_large) = (open(&dxf_small, "x.dxf"), open(&dxf_large, "x.dxf"));
    let steps: [(&str, Duration, Duration); 4] = [
        ("DXF open", fastest(3, || count(&dxf_small, "x.dxf")).0, fastest(3, || count(&dxf_large, "x.dxf")).0),
        ("DWG open", fastest(3, || count(&dwg_small, "x.dwg")).0, fastest(3, || count(&dwg_large, "x.dwg")).0),
        ("DXF save", fastest(3, || save(&again_small, "x.dxf").len()).0, fastest(3, || save(&again_large, "x.dxf").len()).0),
        ("DWG save", fastest(3, || save(&again_small, "x.dwg").len()).0, fastest(3, || save(&again_large, "x.dwg").len()).0),
    ];
    assert_eq!(count(&dwg_large, "x.dwg"), SMALL * FACTOR);
    for (what, a, b) in steps {
        // Below a few milliseconds the ratio is noise.
        let ratio = b.as_secs_f64() / a.as_secs_f64().max(0.005);
        eprintln!("{what}: {SMALL} entities {a:?}, {} entities {b:?}, ratio {ratio:.1}", SMALL * FACTOR);
        assert!(ratio < MAX_RATIO, "{what}: {FACTOR}x the entities took {ratio:.1}x as long ({a:?} → {b:?}), more than {MAX_RATIO}x");
    }
}

/// Writes large drawings for profiling (FF6) into `CADCRAFT_LARGE_OUT`: DXF, DWG (2018 and 2010) of
/// 100,000, 300,000 and 1,000,000 entities (`CADCRAFT_LARGE_SIZES`: other counts, comma-separated). Run on demand:
/// `CADCRAFT_LARGE_OUT=dir cargo test --release -p cadcraft-io --test large_drawing_perf -- --ignored`.
#[test]
#[ignore = "writes hundreds of MB; run on demand for profiling"]
fn write_large_corpus() {
    let Ok(dir) = std::env::var("CADCRAFT_LARGE_OUT") else {
        eprintln!("set CADCRAFT_LARGE_OUT to a folder");
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("create the folder");
    let sizes: Vec<usize> = std::env::var("CADCRAFT_LARGE_SIZES")
        .ok()
        .map(|s| s.split(',').filter_map(|v| v.trim().parse().ok()).collect())
        .unwrap_or_else(|| vec![100_000, 300_000, 1_000_000]);
    for n in sizes {
        let t = Instant::now();
        let d = cadcraft_io::read_dxf(generate(n).as_bytes()).expect("read");
        let read = t.elapsed();
        let t = Instant::now();
        let dxf = cadcraft_io::write(&d, "x.dxf").expect("DXF");
        let save_dxf = t.elapsed();
        let t = Instant::now();
        let dwg = cadcraft_io::write(&d, "x.dwg").expect("DWG");
        let save_dwg = t.elapsed();
        std::fs::write(dir.join(format!("generated-{}k.dxf", n / 1000)), &dxf).expect("write");
        std::fs::write(dir.join(format!("generated-{}k.dwg", n / 1000)), &dwg).expect("write");
        // AutoCAD 2010 (AC1024), the version of the file in #57.
        let opts = cadcraft_io::SaveOptions { version: Some(cadcraft_io::Version::R2010), binary: false };
        let dwg2010 = cadcraft_io::write_with(&d, "x.dwg", &opts).expect("DWG 2010");
        std::fs::write(dir.join(format!("generated-{}k-2010.dwg", n / 1000)), &dwg2010).expect("write");
        eprintln!(
            "{n}: read {read:?}, DXF save {save_dxf:?} ({:.1} MB), DWG save {save_dwg:?} ({:.1} MB)",
            dxf.len() as f64 / 1e6,
            dwg.len() as f64 / 1e6
        );
    }
}

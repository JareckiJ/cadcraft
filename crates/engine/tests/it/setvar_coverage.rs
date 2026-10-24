//! SETVAR/GETVAR cover the variables the commands keep in the drawing header with a default
//! (CIRCLERAD, OFFSETDIST, POLYSIDES, HPNAME…), TRANSPARENCYDISPLAY and the dimension variables in
//! `SETVAR ?`; values are checked; unknown variables still give an error.

use cadcraft_engine::Session;
use serde_json::{Value, json};

fn getvar(s: &mut Session, name: &str) -> Value {
    s.execute("getvar", &json!({ "name": name })).unwrap()["value"].clone()
}

fn setvar(s: &mut Session, name: &str, value: Value) -> bool {
    s.execute("setvar", &json!({ "name": name, "value": value })).is_ok()
}

#[test]
fn defaults_are_the_ones_the_commands_use() {
    let mut s = Session::new();
    for (name, v) in [
        ("POLYSIDES", json!(4)),
        ("OFFSETDIST", json!(-1.0)),
        ("TRIMMODE", json!(1)),
        ("PROJMODE", json!(1)),
        ("EDGEMODE", json!(0)),
        ("HPNAME", json!("ANSI31")),
        ("HPSCALE", json!(1.0)),
        ("HPANG", json!(0.0)),
        ("DONUTID", json!(0.5)),
        ("DONUTOD", json!(1.0)),
        ("WIPEOUTFRAME", json!(1)),
        ("LWDEFAULT", json!(25)),
        ("DIMASSOC", json!(2)),
        ("CPLOTSTYLE", json!("ByLayer")),
        ("TRANSPARENCYDISPLAY", json!(1)),
        ("PLOTTRANSPARENCYOVERRIDE", json!(1)),
    ] {
        assert_eq!(getvar(&mut s, name), v, "{name}");
    }
    // Lowercase names work; the list shows the new variables and the dimension variables.
    assert_eq!(getvar(&mut s, "circlerad"), json!(0.0));
    let all = cadcraft_engine::sysvars::list(&s);
    for n in ["POLYSIDES", "SPLINESEGS", "CHAMFERD", "TRANSPARENCYDISPLAY", "DIMASZ", "OSMODE", "CLAYER"] {
        assert!(all.get(n).is_some(), "{n} listed");
    }
}

#[test]
fn values_reach_the_commands() {
    let mut s = Session::new();
    assert!(setvar(&mut s, "POLYSIDES", json!(6)));
    s.cmdline("polygon").unwrap();
    assert!(s.current_prompt().unwrap().display().contains("<6>"), "{}", s.current_prompt().unwrap().display());
    s.cancel();
    assert!(setvar(&mut s, "HPANG", json!(30)));
    assert_eq!(getvar(&mut s, "HPANG").as_f64().unwrap().round(), 30.0, "degrees in, degrees out");
    assert!((s.doc().unwrap().header.f64("HPANG", 0.0) - 30f64.to_radians()).abs() < 1e-9, "kept in radians");
    // Typed at the command line.
    s.cmdline("setvar hpname net").unwrap();
    assert_eq!(getvar(&mut s, "HPNAME"), json!("NET"));
    s.cmdline("setvar transparencydisplay 0").unwrap();
    assert!(!s.settings.transparency_display);
    s.cmdline("setvar plottransparencyoverride 2").unwrap();
    assert_eq!(s.settings.plot_transparency_override, 2);
    assert!(setvar(&mut s, "INSNAME", json!("Valve")));
    assert!(setvar(&mut s, "INSNAME", json!(".")));
    assert_eq!(getvar(&mut s, "INSNAME"), json!(""));
}

#[test]
fn bad_values_and_unknown_names_are_refused() {
    let mut s = Session::new();
    for (name, v) in [
        ("POLYSIDES", json!(2)),
        ("POLYSIDES", json!(5000)),
        ("TRIMMODE", json!(3)),
        ("LWDEFAULT", json!(26)),
        ("HPSCALE", json!(0)),
        ("HPNAME", json!("NO-SUCH-PATTERN")),
        ("DONUTOD", json!(0)),
        ("CIRCLERAD", json!(-1)),
        ("SPLINESEGS", json!(1e9)),
        ("TRANSPARENCYDISPLAY", json!(5)),
        ("PLOTTRANSPARENCYOVERRIDE", json!(3)),
        ("PLOTTRANSPARENCYOVERRIDE", json!(-1)),
        ("PLOTTRANSPARENCYOVERRIDE", json!("always")),
        ("PLOTTRANSPARENCYOVERRIDE", json!(1.5)),
        ("CHAMFERD", json!("abc")),
        ("CPLOTSTYLE", json!("Style 1")),
        ("OFFSETDIST", json!(f64::MAX.to_string() + "0")),
    ] {
        assert!(!setvar(&mut s, name, v.clone()), "{name} = {v} refused");
    }
    assert_eq!(getvar(&mut s, "POLYSIDES"), json!(4), "unchanged");
    assert!(cadcraft_engine::sysvars::is_read_only("cplotstyle"));
    assert!(s.execute("getvar", &json!({ "name": "NOSUCHVAR" })).is_err());
    s.cmdline("setvar nosuchvar").unwrap();
    assert!(s.log.iter().any(|l| l.contains("Unknown variable NOSUCHVAR")), "{:?}", s.log);
}

#[test]
fn metric_defaults() {
    let mut s = Session::new();
    s.execute("setvar", &json!({ "name": "MEASUREMENT", "value": 1 })).unwrap();
    assert_eq!(getvar(&mut s, "REVCLOUDARCLEN"), json!(15.0));
    assert_eq!(getvar(&mut s, "CMLSCALE"), json!(20.0));
    assert_eq!(getvar(&mut s, "CENTEREXE"), json!(3.5));
    assert_eq!(getvar(&mut s, "DIMTXT"), json!(2.5));
    assert_eq!(getvar(&mut s, "DIMASZ"), json!(2.5));
    assert_eq!(getvar(&mut s, "DIMGAP"), json!(0.625));
    assert_eq!(getvar(&mut s, "DIMEXE"), json!(1.25));
    assert_eq!(getvar(&mut s, "DIMEXO"), json!(0.625));
}

#[test]
fn expanded_coverage_drawing_and_dim_vars() {
    let mut s = Session::new();
    // Test setting and getting entity defaults and scaling
    assert!(setvar(&mut s, "LTSCALE", json!(2.5)));
    assert_eq!(getvar(&mut s, "LTSCALE"), json!(2.5));
    assert!(setvar(&mut s, "CELTSCALE", json!(0.5)));
    assert_eq!(getvar(&mut s, "CELTSCALE"), json!(0.5));
    assert!(setvar(&mut s, "PSLTSCALE", json!(0)));
    assert_eq!(getvar(&mut s, "PSLTSCALE"), json!(0));
    assert!(setvar(&mut s, "CELWEIGHT", json!(50)));
    assert_eq!(getvar(&mut s, "CELWEIGHT"), json!(50));
    assert!(setvar(&mut s, "CELWEIGHT", json!(-1))); // ByLayer
    assert_eq!(getvar(&mut s, "CELWEIGHT"), json!(-1));

    // Point display mode & size
    assert!(setvar(&mut s, "PDMODE", json!(34)));
    assert_eq!(getvar(&mut s, "PDMODE"), json!(34));
    assert!(setvar(&mut s, "PDSIZE", json!(1.5)));
    assert_eq!(getvar(&mut s, "PDSIZE"), json!(1.5));

    // Dimension variables
    assert!(setvar(&mut s, "DIMSCALE", json!(5.0)));
    assert_eq!(getvar(&mut s, "DIMSCALE"), json!(5.0));
    assert!(setvar(&mut s, "DIMTXT", json!(3.0)));
    assert_eq!(getvar(&mut s, "DIMTXT"), json!(3.0));
    assert!(setvar(&mut s, "DIMASZ", json!(2.0)));
    assert_eq!(getvar(&mut s, "DIMASZ"), json!(2.0));
    assert!(setvar(&mut s, "DIMEXE", json!(1.5)));
    assert_eq!(getvar(&mut s, "DIMEXE"), json!(1.5));
    assert!(setvar(&mut s, "DIMEXO", json!(0.8)));
    assert_eq!(getvar(&mut s, "DIMEXO"), json!(0.8));
    assert!(setvar(&mut s, "DIMDLI", json!(4.0)));
    assert_eq!(getvar(&mut s, "DIMDLI"), json!(4.0));
    assert!(setvar(&mut s, "DIMDLE", json!(0.5)));
    assert_eq!(getvar(&mut s, "DIMDLE"), json!(0.5));
    assert!(setvar(&mut s, "DIMLFAC", json!(2.0)));
    assert_eq!(getvar(&mut s, "DIMLFAC"), json!(2.0));
    assert!(setvar(&mut s, "DIMTAD", json!(1)));
    assert_eq!(getvar(&mut s, "DIMTAD"), json!(1));
    assert!(setvar(&mut s, "DIMTIH", json!(0)));
    assert_eq!(getvar(&mut s, "DIMTIH"), json!(0));
    assert!(setvar(&mut s, "DIMTOH", json!(0)));
    assert_eq!(getvar(&mut s, "DIMTOH"), json!(0));
    assert!(setvar(&mut s, "DIMTOFL", json!(1)));
    assert_eq!(getvar(&mut s, "DIMTOFL"), json!(1));
    assert!(setvar(&mut s, "DIMTIX", json!(1)));
    assert_eq!(getvar(&mut s, "DIMTIX"), json!(1));
    assert!(setvar(&mut s, "DIMSOXD", json!(1)));
    assert_eq!(getvar(&mut s, "DIMSOXD"), json!(1));
    assert!(setvar(&mut s, "DIMSE1", json!(1)));
    assert_eq!(getvar(&mut s, "DIMSE1"), json!(1));
    assert!(setvar(&mut s, "DIMSE2", json!(1)));
    assert_eq!(getvar(&mut s, "DIMSE2"), json!(1));
    assert!(setvar(&mut s, "DIMSD1", json!(1)));
    assert_eq!(getvar(&mut s, "DIMSD1"), json!(1));
    assert!(setvar(&mut s, "DIMSD2", json!(1)));
    assert_eq!(getvar(&mut s, "DIMSD2"), json!(1));

    // Bare command line invocation of variable
    s.cmdline("DIMSCALE 10").unwrap();
    assert_eq!(getvar(&mut s, "DIMSCALE"), json!(10.0));

    // In-range and out-of-range validation checks
    assert!(!setvar(&mut s, "LTSCALE", json!(0.0)));
    assert!(!setvar(&mut s, "LTSCALE", json!(-1.0)));
    assert!(!setvar(&mut s, "CELWEIGHT", json!(999)));
    assert!(!setvar(&mut s, "DIMTAD", json!(5)));
    assert!(!setvar(&mut s, "DIMLUNIT", json!(7)));
    assert!(!setvar(&mut s, "DIMLFAC", json!(0.0)));
    assert!(!setvar(&mut s, "DIMLFAC", json!(-2.0)));
}

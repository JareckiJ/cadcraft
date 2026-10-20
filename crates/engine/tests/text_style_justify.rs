//! TEXT's text style and justification after the fact (`properties.set`) and the Style option
//! of the typed TEXT command.

use cadcraft_engine::doc::{EntityKind, HAlign, Handle, Text, VAlign};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};
use serde_json::json;

fn texts(s: &Session) -> Vec<Text> {
    s.doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Text(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn properties_set_changes_a_texts_style_and_justification() {
    let mut s = Session::new();
    s.execute("style", &json!({"name": "Big", "font": "Arial", "current": false})).unwrap();
    let r = s.execute("text", &json!({"at": [10, 20], "text": "HELLO", "height": 2.0})).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap()).unwrap();
    s.execute("properties.set", &json!({"handles": [h.hex()], "style": "big", "justify": "MC"})).unwrap();
    let t = &texts(&s)[0];
    assert_eq!(t.style, "Big");
    assert_eq!((t.halign, t.valign), (HAlign::Center, VAlign::Middle));
    assert_eq!(t.align_pt.map(|p| (p.x, p.y)), Some((10.0, 20.0)), "re-justified about the point it was placed by");
    s.execute("properties.set", &json!({"handles": [h.hex()], "justify": "L"})).unwrap();
    let t = &texts(&s)[0];
    assert_eq!((t.halign, t.valign, t.align_pt), (HAlign::Left, VAlign::Baseline, None));
    assert_eq!((t.insert.x, t.insert.y), (10.0, 20.0));
    for bad in [json!({"handles": [h.hex()], "style": "Nope"}), json!({"handles": [h.hex()], "justify": "XX"})] {
        assert!(s.execute("properties.set", &bad).is_err(), "{bad}");
    }
    assert_eq!(texts(&s)[0].style, "Big", "nothing changed by a refused call");
}

#[test]
fn typed_text_style_option_makes_a_style_current() {
    let mut s = Session::new();
    s.execute("style", &json!({"name": "Big", "font": "Arial", "current": false})).unwrap();
    s.start("text").unwrap();
    assert!(s.log.last().is_some_and(|l| l.contains("Current text style:  \"Standard\"") && l.contains("Justify:  Left")), "{:?}", s.log.last());
    s.input(Input::Keyword("Style".into())).unwrap();
    assert!(s.prompt_text().contains("style name"), "{}", s.prompt_text());
    s.input(Input::Text("?".into())).unwrap();
    assert!(s.log.last().is_some_and(|l| l.contains("Big")), "{:?}", s.log.last());
    assert!(s.prompt_text().contains("style name"), "still asking: {}", s.prompt_text());
    s.input(Input::Text("Nope".into())).unwrap();
    assert!(s.log.last().is_some_and(|l| l.contains("Cannot find text style")), "{:?}", s.log.last());
    assert!(s.prompt_text().contains("style name"), "asked again: {}", s.prompt_text());
    s.input(Input::Text("big".into())).unwrap();
    assert!(s.prompt_text().contains("start point"), "{}", s.prompt_text());
    assert_eq!(s.doc().unwrap().header.str("TEXTSTYLE", ""), "Big", "the Style option makes the style current");
    assert!(s.log.last().is_some_and(|l| l.contains("Current text style:  \"Big\"")), "the status line again: {:?}", s.log.last());
    s.input(Input::Keyword("Justify".into())).unwrap();
    s.input(Input::Keyword("TR".into())).unwrap();
    assert!(s.log.last().is_some_and(|l| l.contains("Justify:  TR")), "{:?}", s.log.last());
    s.input(Input::Point(Vec2::new(1.0, 2.0))).unwrap();
    s.input(Input::Text("2".into())).unwrap();
    s.input(Input::Enter).unwrap();
    s.input(Input::Text("styled".into())).unwrap();
    let t = &texts(&s)[0];
    assert_eq!((t.style.as_str(), t.value.as_str()), ("Big", "styled"));
    assert_eq!((t.halign, t.valign), (HAlign::Right, VAlign::Top));
    // Enter at the Style question keeps the current style.
    s.input(Input::Enter).unwrap();
    s.start("text").unwrap();
    s.input(Input::Keyword("Style".into())).unwrap();
    s.input(Input::Enter).unwrap();
    assert!(s.prompt_text().contains("start point"), "{}", s.prompt_text());
    assert_eq!(s.doc().unwrap().header.str("TEXTSTYLE", ""), "Big");
}

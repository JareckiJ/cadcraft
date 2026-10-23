//! IEC 81346-2 object classes: the letter code of a component, read from a tag someone already
//! typed (`-K`, `=A1+M1-QA3`) or guessed from its block's name (`RELAY_24V` → `K`).
//!
//! The keyword table is our own list of everyday component words mapped to the main classes of
//! IEC 81346-2:2019 (the class letters are a public standard; no vendor library is copied).

/// The main classes of IEC 81346-2:2019 by letter, with what they mean, for reports.
pub const MAIN_CLASSES: &[(char, &str)] = &[
    ('A', "Two or more purposes"),
    ('B', "Converting an input variable into a signal (sensors)"),
    ('C', "Storing energy, information or material"),
    ('E', "Providing radiant or thermal energy"),
    ('F', "Protecting against dangerous or unwanted conditions"),
    ('G', "Initiating a flow of energy or material (generators)"),
    ('H', "Producing a new kind of material or product"),
    ('K', "Processing signals or information (relays, controllers)"),
    ('M', "Providing mechanical energy (motors, actuators)"),
    ('P', "Presenting information (lamps, meters)"),
    ('Q', "Controlled switching of a flow of energy (contactors, breakers)"),
    ('R', "Restricting or stabilizing a flow of energy"),
    ('S', "Converting a manual action into a signal (push-buttons)"),
    ('T', "Converting energy keeping its kind (transformers, power supplies)"),
    ('U', "Keeping objects in a defined position"),
    ('V', "Processing (treating) material or products"),
    ('W', "Guiding or transporting energy or signals (cables, conductors)"),
    ('X', "Connecting objects (terminals, plugs, sockets)"),
];

/// Whether `c` is one of the IEC 81346-2 main class letters.
pub fn is_main_class(c: char) -> bool {
    MAIN_CLASSES.iter().any(|(l, _)| *l == c)
}

/// Words found in block names, mapped to a main class. Longer words are tried first, so
/// "limitswitch" (a sensor, B) wins over "switch" (manual, S).
const KEYWORDS: &[(&str, &str)] = &[
    // K: processing signals
    ("relay", "K"),
    ("coil", "K"),
    ("plc", "K"),
    ("controller", "K"),
    ("timer", "K"),
    ("safetyrelay", "K"),
    // Q: switching energy
    ("contactor", "Q"),
    ("breaker", "Q"),
    ("mcb", "Q"),
    ("mccb", "Q"),
    ("isolator", "Q"),
    ("disconnector", "Q"),
    ("disconnect", "Q"),
    ("mainswitch", "Q"),
    ("starter", "Q"),
    ("softstarter", "Q"),
    ("vfd", "Q"),
    ("inverter", "Q"),
    // F: protection
    ("fuse", "F"),
    ("rcd", "F"),
    ("rcbo", "F"),
    ("overload", "F"),
    ("surge", "F"),
    ("spd", "F"),
    // M: mechanical energy
    ("motor", "M"),
    ("actuator", "M"),
    ("fan", "M"),
    ("pump", "M"),
    // S: manual action to signal
    ("pushbutton", "S"),
    ("button", "S"),
    ("estop", "S"),
    ("emergencystop", "S"),
    ("selector", "S"),
    ("switch", "S"),
    ("keyswitch", "S"),
    // B: sensing
    ("sensor", "B"),
    ("limitswitch", "B"),
    ("proximity", "B"),
    ("prox", "B"),
    ("encoder", "B"),
    ("thermostat", "B"),
    ("pressureswitch", "B"),
    ("flowswitch", "B"),
    ("levelswitch", "B"),
    ("thermocouple", "B"),
    ("currenttransformer", "B"),
    // P: presenting information
    ("lamp", "P"),
    ("pilotlight", "P"),
    ("indicator", "P"),
    ("meter", "P"),
    ("ammeter", "P"),
    ("voltmeter", "P"),
    ("display", "P"),
    ("hmi", "P"),
    ("buzzer", "P"),
    ("horn", "P"),
    ("beacon", "P"),
    // T: converting energy
    ("transformer", "T"),
    ("psu", "T"),
    ("powersupply", "T"),
    ("converter", "T"),
    // X: connecting
    ("terminal", "X"),
    ("plug", "X"),
    ("socket", "X"),
    ("connector", "X"),
    // W: guiding
    ("cable", "W"),
    ("busbar", "W"),
    // R, C, E, G
    ("resistor", "R"),
    ("diode", "R"),
    ("choke", "R"),
    ("capacitor", "C"),
    ("battery", "C"),
    ("heater", "E"),
    ("generator", "G"),
];

/// The class letters at the start of the last `-` part of `tag` (`-K`, `-K?`, `=A1+M2-QA12` →
/// `QA`), at most three capital letters, the first a main class. `None` when it has none.
pub fn class_from_tag(tag: &str) -> Option<String> {
    let product = tag.rsplit('-').next().unwrap_or(tag).trim();
    let letters: String = product.chars().take_while(|c| c.is_ascii_uppercase()).take(4).collect();
    let first = letters.chars().next()?;
    (letters.len() <= 3 && is_main_class(first)).then_some(letters)
}

/// The class a block called `name` most likely belongs to, by the longest keyword its name
/// contains (letters and digits only, any case).
pub fn class_from_block_name(name: &str) -> Option<&'static str> {
    let n: String = name.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect();
    KEYWORDS.iter().filter(|(k, _)| n.contains(k)).max_by_key(|(k, _)| k.len()).map(|(_, c)| *c)
}

/// Whether `tag` is a finished tag: it has a number after its class letters and no `?`.
pub fn is_numbered(tag: &str) -> bool {
    let product = tag.rsplit('-').next().unwrap_or(tag);
    !tag.contains('?') && product.chars().skip_while(|c| c.is_ascii_uppercase()).any(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_from_tags() {
        assert_eq!(class_from_tag("-K").as_deref(), Some("K"));
        assert_eq!(class_from_tag("-K?").as_deref(), Some("K"));
        assert_eq!(class_from_tag("=A1+M2-QA12").as_deref(), Some("QA"));
        assert_eq!(class_from_tag("K3").as_deref(), Some("K"));
        assert_eq!(class_from_tag(""), None);
        assert_eq!(class_from_tag("-ZZ"), None, "Z is not a main class");
        assert_eq!(class_from_tag("-KFAB1"), None, "too many letters");
        assert_eq!(class_from_tag("-12"), None);
    }

    #[test]
    fn classes_from_block_names() {
        assert_eq!(class_from_block_name("RELAY_24V_DC"), Some("K"));
        assert_eq!(class_from_block_name("Limit Switch NO"), Some("B"));
        assert_eq!(class_from_block_name("Selector-Switch-3Pos"), Some("S"));
        assert_eq!(class_from_block_name("MAIN_SWITCH"), Some("Q"));
        assert_eq!(class_from_block_name("Motor 3ph"), Some("M"));
        assert_eq!(class_from_block_name("fuse"), Some("F"));
        assert_eq!(class_from_block_name("WINDOW_FRAME"), None);
    }

    #[test]
    fn numbered_tags() {
        assert!(is_numbered("-K1"));
        assert!(is_numbered("-K2.4"));
        assert!(is_numbered("=A1-QA3"));
        assert!(!is_numbered("-K"));
        assert!(!is_numbered("-K?"));
        assert!(!is_numbered("-K1?"));
        assert!(!is_numbered(""));
    }
}

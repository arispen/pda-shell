//! A small iCalendar (RFC 5545) reader and writer: content lines (folding,
//! parameters, text escaping), nested components, and the date/time value
//! forms. Properties it does not know are kept as they are, so a file
//! written by another program keeps its RRULE, VALARM, VTIMEZONE, ... when
//! the PDA rewrites it.

use super::date::{Date, DateTime};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Property {
    /// Upper case ("DTSTART").
    pub name: String,
    /// Parameters, names upper case ("TZID", "Europe/Zurich").
    pub params: Vec<(String, String)>,
    /// The value as written (escaped for TEXT values).
    pub value: String,
}

impl Property {
    pub fn new(name: &str, value: impl Into<String>) -> Self {
        Self { name: name.into(), params: Vec::new(), value: value.into() }
    }

    pub fn text(name: &str, text: &str) -> Self {
        Self::new(name, escape(text))
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Component {
    /// "VCALENDAR", "VEVENT", "VTODO", ...
    pub kind: String,
    pub props: Vec<Property>,
    pub children: Vec<Component>,
}

impl Component {
    pub fn new(kind: &str) -> Self {
        Self { kind: kind.into(), ..Default::default() }
    }

    pub fn prop(&self, name: &str) -> Option<&Property> {
        self.props.iter().find(|p| p.name == name)
    }

    /// An unescaped TEXT property.
    pub fn text(&self, name: &str) -> Option<String> {
        self.prop(name).map(|p| unescape(&p.value))
    }

    /// Replace (or with `None` remove) every `name` property.
    pub fn set(&mut self, name: &str, p: Option<Property>) {
        let at = self.props.iter().position(|q| q.name == name);
        self.props.retain(|q| q.name != name);
        if let Some(p) = p {
            let at = at.unwrap_or(self.props.len()).min(self.props.len());
            self.props.insert(at, p);
        }
    }

    pub fn set_text(&mut self, name: &str, text: &str) {
        self.set(name, (!text.is_empty()).then(|| Property::text(name, text)));
    }
}

/// TEXT value escaping (RFC 5545 3.3.11).
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

pub fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(c) => out.push(c),
            None => out.push('\\'),
        }
    }
    out
}

/// Split a content line into name, parameters and value. Quoted parameter
/// values may contain ':' ';' ','.
fn parse_line(line: &str) -> Option<Property> {
    let mut in_quotes = false;
    let mut colon = None;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            ':' if !in_quotes => {
                colon = Some(i);
                break;
            }
            _ => {}
        }
    }
    let colon = colon?;
    let (head, value) = (&line[..colon], &line[colon + 1..]);
    let mut parts = Vec::new();
    let (mut start, mut in_quotes) = (0, false);
    for (i, c) in head.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            ';' if !in_quotes => {
                parts.push(&head[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&head[start..]);
    let name = parts[0].trim().to_ascii_uppercase();
    if name.is_empty() {
        return None;
    }
    let params = parts[1..]
        .iter()
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.trim().to_ascii_uppercase(), v.trim_matches('"').to_string()))
        .collect();
    Some(Property { name, params, value: value.to_string() })
}

/// The top-level components of an iCalendar stream (usually one
/// VCALENDAR). Lenient about line endings and case; errors on structure
/// that cannot be read.
pub fn parse(text: &str) -> Result<Vec<Component>, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    // Unfold: a line starting with a space or tab continues the previous one.
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(cont) = raw.strip_prefix([' ', '\t']) {
            match lines.last_mut() {
                Some(last) => last.push_str(cont),
                None => return Err("the file starts with a continuation line".into()),
            }
        } else if !raw.trim().is_empty() {
            lines.push(raw.to_string());
        }
    }
    let mut stack: Vec<Component> = Vec::new();
    let mut top = Vec::new();
    for (n, line) in lines.iter().enumerate() {
        let p = parse_line(line).ok_or_else(|| format!("line {}: not a content line", n + 1))?;
        match p.name.as_str() {
            "BEGIN" => stack.push(Component::new(&p.value.trim().to_ascii_uppercase())),
            "END" => {
                let c = stack.pop().ok_or_else(|| format!("line {}: END without BEGIN", n + 1))?;
                if !c.kind.eq_ignore_ascii_case(p.value.trim()) {
                    return Err(format!("line {}: END:{} closes {}", n + 1, p.value.trim(), c.kind));
                }
                match stack.last_mut() {
                    Some(parent) => parent.children.push(c),
                    None => top.push(c),
                }
            }
            _ => match stack.last_mut() {
                Some(c) => c.props.push(p),
                None => return Err(format!("line {}: property outside a component", n + 1)),
            },
        }
    }
    if let Some(c) = stack.last() {
        return Err(format!("{} is not closed", c.kind));
    }
    Ok(top)
}

fn push_folded(out: &mut String, line: &str) {
    // Fold at 75 octets, never inside a UTF-8 character.
    let mut width = 0;
    for c in line.chars() {
        let n = c.len_utf8();
        if width + n > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width += n;
    }
    out.push_str("\r\n");
}

fn write_component(out: &mut String, c: &Component) {
    push_folded(out, &format!("BEGIN:{}", c.kind));
    for p in &c.props {
        let mut line = p.name.clone();
        for (k, v) in &p.params {
            let quote = v.contains([':', ';', ',']);
            line.push_str(&if quote { format!(";{k}=\"{v}\"") } else { format!(";{k}={v}") });
        }
        line.push(':');
        line.push_str(&p.value);
        push_folded(out, &line);
    }
    for child in &c.children {
        write_component(out, child);
    }
    push_folded(out, &format!("END:{}", c.kind));
}

/// The component as iCalendar text (CRLF line ends, folded lines).
pub fn write(c: &Component) -> String {
    let mut out = String::new();
    write_component(&mut out, c);
    out
}

/// A DTSTART / DTEND / DUE value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum When {
    /// VALUE=DATE: a whole day.
    Date(Date),
    /// No zone: the same wall-clock time wherever the device is.
    Floating(DateTime),
    /// UTC ("...Z").
    Utc(DateTime),
    /// A named zone (TZID parameter).
    Zoned(DateTime, String),
}

impl When {
    pub fn from_property(p: &Property) -> Option<Self> {
        let v = p.value.trim();
        if p.param("VALUE").is_some_and(|t| t.eq_ignore_ascii_case("DATE")) || v.len() == 8 {
            return Date::parse_ical(v).map(When::Date);
        }
        let t = DateTime::parse_ical(v)?;
        Some(if v.ends_with('Z') {
            When::Utc(t)
        } else if let Some(tz) = p.param("TZID") {
            When::Zoned(t, tz.to_string())
        } else {
            When::Floating(t)
        })
    }

    pub fn to_property(&self, name: &str) -> Property {
        match self {
            When::Date(d) => Property { name: name.into(), params: vec![("VALUE".into(), "DATE".into())], value: d.ical() },
            When::Floating(t) => Property::new(name, t.ical()),
            When::Utc(t) => Property::new(name, format!("{}Z", t.ical())),
            When::Zoned(t, tz) => Property { name: name.into(), params: vec![("TZID".into(), tz.clone())], value: t.ical() },
        }
    }

    /// The local wall-clock time (midnight for a date).
    pub fn local(&self, zones: &dyn Zones) -> DateTime {
        match self {
            When::Date(d) => d.at(0, 0),
            When::Floating(t) => *t,
            When::Utc(t) => zones.to_local(*t, None),
            When::Zoned(t, tz) => zones.to_local(*t, Some(tz)),
        }
    }
}

/// Converts zoned times to the device's local time. The programs use
/// GLib's time zone database; tests use fixed offsets.
pub trait Zones {
    /// `tz` None means UTC.
    fn to_local(&self, t: DateTime, tz: Option<&str>) -> DateTime;
    /// The current local time.
    fn now(&self) -> DateTime;
    /// The current UTC time.
    fn now_utc(&self) -> DateTime;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_escaping_round_trips() {
        let s = "Lunch; bring a, b\\c\nline two";
        assert_eq!(escape(s), "Lunch\\; bring a\\, b\\\\c\\nline two");
        assert_eq!(unescape(&escape(s)), s);
    }

    #[test]
    fn parses_folded_lines_params_and_nesting() {
        let text = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nSUMMARY:A very long \r\n title\r\nDTSTART;TZID=\"Europe/Zurich\":20261027T090000\r\nX-THING;A=\"x:y;z\":v:w\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let cals = parse(text).unwrap();
        let ev = &cals[0].children[0];
        assert_eq!(ev.text("SUMMARY").unwrap(), "A very long title");
        let start = ev.prop("DTSTART").unwrap();
        assert_eq!(start.param("TZID"), Some("Europe/Zurich"));
        assert_eq!(ev.prop("X-THING").unwrap().value, "v:w");
        assert_eq!(ev.prop("X-THING").unwrap().param("A"), Some("x:y;z"));
        assert_eq!(ev.children[0].kind, "VALARM");
        // Writing and reading again gives the same components.
        assert_eq!(parse(&write(&cals[0])).unwrap(), cals);
    }

    #[test]
    fn folding_keeps_utf8_intact() {
        let mut c = Component::new("VTODO");
        let long = "Zażółć gęślą jaźń ".repeat(10);
        c.set_text("SUMMARY", &long);
        let text = write(&c);
        assert!(text.lines().all(|l| l.trim_end_matches('\r').len() <= 75));
        assert_eq!(parse(&text).unwrap()[0].text("SUMMARY").unwrap(), long);
    }

    #[test]
    fn malformed_input_is_an_error_not_a_panic() {
        for bad in ["BEGIN:VEVENT\nSUMMARY:x\n", "END:VEVENT\n", "SUMMARY:x\n", " cont\n", "BEGIN:A\nEND:B\n", "BEGIN:A\nno colon\nEND:A"] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
        assert!(parse("").unwrap().is_empty());
    }

    #[test]
    fn when_forms() {
        let p = |s: &str| parse_line(s).unwrap();
        assert_eq!(When::from_property(&p("DTSTART;VALUE=DATE:20261027")), Some(When::Date(Date::new(2026, 10, 27).unwrap())));
        assert!(matches!(When::from_property(&p("DTSTART:20261027T090000Z")), Some(When::Utc(_))));
        assert!(matches!(When::from_property(&p("DTSTART;TZID=Europe/Zurich:20261027T090000")), Some(When::Zoned(_, _))));
        assert!(matches!(When::from_property(&p("DTSTART:20261027T090000")), Some(When::Floating(_))));
        assert_eq!(When::from_property(&p("DTSTART:garbage")), None);
        for w in ["DTSTART;VALUE=DATE:20261027", "DTSTART:20261027T090000Z", "DTSTART;TZID=Europe/Zurich:20261027T090000"] {
            assert_eq!(When::from_property(&p(w)).unwrap().to_property("DTSTART"), p(w));
        }
    }
}

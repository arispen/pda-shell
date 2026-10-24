//! Session-wide settings for the programs the shell starts: the shell acts
//! as the XSETTINGS manager (read by GTK 3 and GTK 4, and through GTK by
//! Firefox) and also sets the `Xft.dpi` X resource for toolkits that only
//! read resources. Nothing is written to the user's files; the settings live
//! as long as the shell's X connection.
//!
//! XSETTINGS: <https://specifications.freedesktop.org/xsettings-spec/latest/>

/// A setting value. Colors are not needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Setting {
    Int(i32),
    Str(String),
}

fn pad4(buf: &mut Vec<u8>) {
    while buf.len() % 4 != 0 {
        buf.push(0);
    }
}

/// The `_XSETTINGS_SETTINGS` property value, little-endian.
pub fn xsettings_blob(serial: u32, settings: &[(&str, Setting)]) -> Vec<u8> {
    let mut b = vec![0u8, 0, 0, 0]; // byte order LSBFirst, 3 bytes padding
    b.extend(serial.to_le_bytes());
    b.extend((settings.len() as u32).to_le_bytes());
    for (name, value) in settings {
        let kind = match value {
            Setting::Int(_) => 0u8,
            Setting::Str(_) => 1u8,
        };
        b.push(kind);
        b.push(0);
        b.extend((name.len() as u16).to_le_bytes());
        b.extend(name.as_bytes());
        pad4(&mut b);
        b.extend(serial.to_le_bytes()); // last-change serial
        match value {
            Setting::Int(i) => b.extend(i.to_le_bytes()),
            Setting::Str(s) => {
                b.extend((s.len() as u32).to_le_bytes());
                b.extend(s.as_bytes());
                pad4(&mut b);
            }
        }
    }
    b
}

/// The settings for a font resolution and default font.
pub fn session_settings(dpi: u32, font: &str) -> Vec<(&'static str, Setting)> {
    let mut v = vec![("Xft/DPI", Setting::Int((dpi * 1024) as i32))];
    if !font.is_empty() {
        v.push(("Gtk/FontName", Setting::Str(font.to_string())));
    }
    v
}

/// `RESOURCE_MANAGER` contents with `entries` set, other lines kept.
pub fn merge_resources(existing: &str, entries: &[(&str, String)]) -> String {
    let mut out: Vec<String> = existing
        .lines()
        .filter(|l| {
            let key = l.split(':').next().unwrap_or("").trim();
            !l.trim().is_empty() && !entries.iter().any(|(k, _)| *k == key)
        })
        .map(String::from)
        .collect();
    out.extend(entries.iter().map(|(k, v)| format!("{k}:\t{v}")));
    out.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_layout() {
        let b = xsettings_blob(7, &[("Xft/DPI", Setting::Int(144 * 1024)), ("Gtk/FontName", Setting::Str("Tahoma 10".into()))]);
        assert_eq!(&b[0..4], &[0, 0, 0, 0]);
        assert_eq!(u32::from_le_bytes(b[4..8].try_into().unwrap()), 7);
        assert_eq!(u32::from_le_bytes(b[8..12].try_into().unwrap()), 2);
        // First setting: type 0, name length 7, name padded to 8, serial, value.
        assert_eq!(b[12], 0);
        assert_eq!(u16::from_le_bytes(b[14..16].try_into().unwrap()), 7);
        assert_eq!(&b[16..23], b"Xft/DPI");
        assert_eq!(u32::from_le_bytes(b[24..28].try_into().unwrap()), 7);
        assert_eq!(i32::from_le_bytes(b[28..32].try_into().unwrap()), 147456);
        // Second: type 1, 12-byte name, string length 9 padded to 12.
        assert_eq!(b[32], 1);
        assert_eq!(&b[36..48], b"Gtk/FontName");
        assert_eq!(u32::from_le_bytes(b[52..56].try_into().unwrap()), 9);
        assert_eq!(&b[56..65], b"Tahoma 10");
        assert_eq!(b.len(), 68);
        assert_eq!(b.len() % 4, 0);
    }

    #[test]
    fn dpi_is_in_1024ths() {
        assert_eq!(session_settings(96, "")[0], ("Xft/DPI", Setting::Int(98304)));
        assert_eq!(session_settings(144, "Tahoma 10").len(), 2);
    }

    #[test]
    fn resources_are_merged() {
        let merged = merge_resources("Xft.dpi:\t96\nXcursor.size: 24\n", &[("Xft.dpi", "144".into())]);
        assert_eq!(merged, "Xcursor.size: 24\nXft.dpi:\t144\n");
        assert_eq!(merge_resources("", &[("Xft.dpi", "120".into())]), "Xft.dpi:\t120\n");
    }
}

//! Text focus in any program, from the accessibility bus (AT-SPI): used to
//! show the input panel when an editable field gets the focus
//! (`[keyboard] show_on_focus`).
//!
//! The shell registers for `object:state-changed:focused` with the AT-SPI
//! registry (programs only send events someone listens for), and asks each
//! newly focused object for its state: an EDITABLE object is a text field.
//! GTK 3 and GTK 4 programs report this; Firefox only when accessibility
//! is enabled (`GNOME_ACCESSIBILITY=1`), and programs without AT-SPI
//! support (plain X11) never do: the keyboard button stays the way to open
//! the panel everywhere.

use gtk::gio;
use gtk::prelude::*;
use std::rc::Rc;

/// ATSPI_STATE_EDITABLE, a bit of the first word of GetState.
const STATE_EDITABLE: u32 = 7;

/// Whether an AT-SPI state set (as returned by GetState) is editable.
pub fn is_editable(states: &[u32]) -> bool {
    states.first().is_some_and(|w| w & (1 << STATE_EDITABLE) != 0)
}

/// A focus change: which object (bus name, path) and whether it is a text
/// field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Focus {
    pub object: (String, String),
    pub editable: bool,
}

/// What to do with a focus change, given whether the panel is shown and
/// the field the user last dismissed the panel in (manual override).
pub fn decide(focus: &Focus, panel_visible: bool, dismissed_in: Option<&(String, String)>) -> bool {
    focus.editable && !panel_visible && dismissed_in != Some(&focus.object)
}

/// Keeps the AT-SPI connection and subscription alive.
pub struct FocusWatch {
    _conn: gio::DBusConnection,
    _sub: gio::SignalSubscription,
}

/// Start following focus; `on_focus` runs on the main loop for every
/// object that gains the focus.
pub fn watch(on_focus: Rc<dyn Fn(Focus)>) -> Option<FocusWatch> {
    let session = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).ok()?;
    let reply = session
        .call_sync(
            Some("org.a11y.Bus"),
            "/org/a11y/bus",
            "org.a11y.Bus",
            "GetAddress",
            None,
            Some(glib::VariantTy::new("(s)").ok()?),
            gio::DBusCallFlags::NONE,
            2000,
            gio::Cancellable::NONE,
        )
        .map_err(|e| log::warn!("no accessibility bus, keyboard-on-focus is off: {e}"))
        .ok()?;
    let (address,) = reply.get::<(String,)>()?;
    let conn = gio::DBusConnection::for_address_sync(
        &address,
        gio::DBusConnectionFlags::AUTHENTICATION_CLIENT | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
        None::<&gio::DBusAuthObserver>,
        gio::Cancellable::NONE,
    )
    .map_err(|e| log::warn!("cannot connect to the accessibility bus: {e}"))
    .ok()?;
    let sub = conn.subscribe_to_signal(
        None,
        Some("org.a11y.atspi.Event.Object"),
        Some("StateChanged"),
        None,
        Some("focused"),
        gio::DBusSignalFlags::NONE,
        {
            let conn = conn.clone();
            move |signal| {
                // (kind, detail1, detail2, any_data, properties); detail1 = 1: gained.
                let gained = signal.parameters.child_value(1).get::<i32>() == Some(1);
                if !gained {
                    return;
                }
                let object = (signal.sender_name.to_string(), signal.object_path.to_string());
                let on_focus = on_focus.clone();
                let (name, path) = object.clone();
                conn.call(
                    Some(&name),
                    &path,
                    "org.a11y.atspi.Accessible",
                    "GetState",
                    None,
                    glib::VariantTy::new("(au)").ok(),
                    gio::DBusCallFlags::NONE,
                    1000,
                    gio::Cancellable::NONE,
                    move |r| {
                        let editable = r.ok().and_then(|v| v.get::<(Vec<u32>,)>()).is_some_and(|(s,)| is_editable(&s));
                        on_focus(Focus { object, editable });
                    },
                );
            }
        },
    );
    // Programs send focus events only when a listener is registered.
    conn.call(
        Some("org.a11y.atspi.Registry"),
        "/org/a11y/atspi/registry",
        "org.a11y.atspi.Registry",
        "RegisterEvent",
        Some(&("object:state-changed:focused", Vec::<String>::new(), "").to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        2000,
        gio::Cancellable::NONE,
        |r| match r {
            Ok(_) => log::info!("following text focus over AT-SPI"),
            Err(e) => log::warn!("AT-SPI registry refused the focus listener: {e}"),
        },
    );
    Some(FocusWatch { _conn: conn, _sub: sub })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn focus(path: &str, editable: bool) -> Focus {
        Focus { object: (":1.5".into(), path.into()), editable }
    }

    #[test]
    fn editable_bit() {
        assert!(is_editable(&[1 << 7, 0]));
        assert!(!is_editable(&[1 << 6, 1 << 7]));
        assert!(!is_editable(&[]));
    }

    #[test]
    fn shows_for_new_text_fields_only() {
        let field = focus("/a/1", true);
        assert!(decide(&field, false, None));
        assert!(!decide(&field, true, None), "already shown");
        assert!(!decide(&focus("/a/2", false), false, None), "a button");
        // Dismissed by hand in this field: stays hidden until another field.
        let dismissed = field.object.clone();
        assert!(!decide(&field, false, Some(&dismissed)));
        assert!(decide(&focus("/a/3", true), false, Some(&dismissed)));
    }
}

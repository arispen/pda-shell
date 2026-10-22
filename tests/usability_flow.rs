//! The manual usability script from the POC brief, replayed against the
//! shell state machine with simulated window-manager notifications. This
//! checks the shell's decisions; scripts/smoke-test.sh checks the same flow
//! against a real X server, Openbox and real applications.

use pda_shell::platform::{AppWindow, WindowId, WindowSnapshot};
use pda_shell::shell::state::{Effect, Event, ShellState, View};

fn win(id: u32, title: &str, class: &str) -> AppWindow {
    AppWindow { id: WindowId(id), title: title.into(), app_class: class.into(), pid: Some(id) }
}

fn windows(list: &[&AppWindow], active: Option<u32>, showing_desktop: bool) -> Event {
    Event::Windows(WindowSnapshot {
        windows: list.iter().map(|w| (*w).clone()).collect(),
        active: active.map(WindowId),
        showing_desktop,
    })
}

#[test]
fn files_spreadsheet_editor_today_round_trip() {
    let mut s = ShellState::new();
    assert_eq!(s.view(), View::Today);

    // Launch Thunar from Start.
    assert_eq!(s.handle(Event::StartButton), vec![Effect::ShowStartMenu(true)]);
    let fx = s.handle(Event::Launch("thunar.desktop".into()));
    assert!(fx.contains(&Effect::Launch("thunar.desktop".into())));
    let thunar = win(1, "Documents - Thunar", "Thunar");
    s.handle(windows(&[&thunar], Some(1), false));
    assert_eq!(s.title(), "Documents - Thunar");

    // Open the spreadsheet from Thunar: Gnumeric maps and becomes active.
    let gnumeric = win(2, "budget.csv : Gnumeric", "Gnumeric");
    s.handle(windows(&[&thunar, &gnumeric], Some(2), false));
    assert_eq!(s.view(), View::App(WindowId(2)));

    // Keyboard on, edit, keyboard off.
    assert_eq!(s.handle(Event::ToggleKeyboard), vec![Effect::SetKeyboard(true)]);
    assert_eq!(s.handle(Event::ToggleKeyboard), vec![Effect::SetKeyboard(false)]);

    // Start Mousepad; it appears while an app is in front and is focused by
    // the window manager itself.
    s.handle(Event::Launch("org.xfce.mousepad.desktop".into()));
    let mousepad = win(3, "notes.txt - Mousepad", "Mousepad");
    let fx = s.handle(windows(&[&thunar, &gnumeric, &mousepad], Some(3), false));
    assert!(fx.is_empty());

    // Back to the spreadsheet through the switcher: Gnumeric is second.
    let order: Vec<u32> = s.switcher_entries().iter().map(|w| w.id.0).collect();
    assert_eq!(order, vec![3, 2, 1]);
    assert_eq!(s.handle(Event::Switch(WindowId(2))), vec![Effect::Activate(WindowId(2))]);
    s.handle(windows(&[&thunar, &gnumeric, &mousepad], Some(2), false));
    assert_eq!(s.title(), "budget.csv : Gnumeric");

    // Return to Today; the applications keep running.
    assert_eq!(s.handle(Event::Today), vec![Effect::ShowDesktop(true)]);
    s.handle(windows(&[&thunar, &gnumeric, &mousepad], None, true));
    assert_eq!(s.view(), View::Today);
    assert_eq!(s.switcher_entries().len(), 3);

    // Open and close the Start menu without launching anything.
    s.handle(Event::StartButton);
    assert_eq!(s.handle(Event::DismissStartMenu), vec![Effect::ShowStartMenu(false)]);
    assert_eq!(s.view(), View::Today);
}

// pda-shell: per-program scaling exception for Firefox ESR, installed into
// /etc/firefox-esr/ by scripts/setup-debian.sh (Debian's Firefox reads every
// .js file there as default preferences; users can still change it).
//
// The shell publishes 144 dpi to programs (docs/ergonomics.md). On X11
// Firefox keeps one device pixel per CSS pixel and turns the DPI into a text
// scale of 150 %, which makes its toolbars need at least 760 px: wider than
// the 600 px portrait screen. Measured minimum widths: 625 px at 125 %, fits
// at 115 %. Page text at 115 % is 18 px (1.9 mm on the 5" panel).
pref("ui.textScaleFactor", 115);

// Compatibility with the shell (issue #4, P1):
// - no "Privacy Notice" tab on first start; it opened in every new session;
// - no window buttons in the tab bar: the shell's command bar closes
//   programs and the window manager keeps windows maximized.
pref("datareporting.policy.firstRunURL", "");
pref("datareporting.policy.dataSubmissionPolicyBypassNotification", true);
pref("browser.tabs.inTitlebar", 0);

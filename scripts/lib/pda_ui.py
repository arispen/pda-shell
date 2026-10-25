#!/usr/bin/env python3
"""Pointer-only driver for end-to-end tests of the PDA session.

Everything this tool does to the session is a pointer tap (xdotool mouse
moves and clicks), the same thing a stylus does:

  osk-type TEXT          type TEXT by tapping keys of the on-screen keyboard
                         (Onboard). {Name} taps a named key: {Return} {Tab}
                         {Esc} {BackSpace} {Delete} {Left} {Right} {Up}
                         {Down} {Ctrl} {Alt} {Shift}; {Ctrl}s is Ctrl+S.
  osk-keys               print the computed key map (debugging)
  find APP ROLE NAME     print "x y w h" of the first visible accessible
                         object of APP with ROLE whose name matches the
                         regular expression NAME (exit 1 if none)
  tap APP ROLE NAME      find, then tap the centre of the object
  wait APP ROLE NAME     wait for the object (default 10 s)
  text APP ROLE NAME     print the text content of the object
  long-press X Y [MS]    hold the pointer still at X,Y for MS (default 900)

  Options for find/tap/wait/text: --frame RE (only inside windows whose
  title matches), --timeout S (wait up to S seconds, also for find/tap),
  --pick leftmost|topmost (among all matches, instead of the first).

AT-SPI is only used to *find* things on screen (like eyes), never to
activate them. Key positions come from Onboard's own layout code, fitted to
the size of the running Onboard window, so the taps land where Onboard
draws its keys.

Must run inside the session (DISPLAY and DBUS_SESSION_BUS_ADDRESS set).
"""
import os
import re
import subprocess
import sys
import time

TAP_DELAY = float(os.environ.get("PDA_TAP_DELAY", "0.12"))
# GTK 4 on X11 reports positions relative to the toplevel window only; the
# window's own position comes from the X server.
GTK4_APPS = {"pda-shell", "pda-files", "pda-notes"}


def xdotool(*args):
    subprocess.run(["xdotool", *map(str, args)], check=True)


def tap(x, y, settle=0.0):
    xdotool("mousemove", int(x), int(y))
    time.sleep(0.05)
    xdotool("click", 1)
    time.sleep(TAP_DELAY + settle)


# Onboard redraws the whole keyboard when the layer changes; a tap that
# arrives before that lands on the old layer.
LAYER_SETTLE = 0.35


# --- AT-SPI ---------------------------------------------------------------------

def atspi():
    import gi
    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi
    return Atspi


def window_origin(title):
    """Screen position of the visible X window called `title`."""
    try:
        ids = subprocess.run(["xdotool", "search", "--onlyvisible", "--name", f"^{re.escape(title)}$"],
                             capture_output=True, text=True).stdout.split()
        out = subprocess.run(["xdotool", "getwindowgeometry", ids[0]], capture_output=True, text=True).stdout
        x, y = re.search(r"Position: (-?\d+),(-?\d+)", out).groups()
        return int(x), int(y)
    except (IndexError, AttributeError):
        return None


def shown_name(o, depth=0):
    """The accessible name, or the text of the first label inside (GTK 4
    popover-menu items have no name of their own)."""
    name = o.get_name() or ""
    if name or depth > 3:
        return name
    for i in range(o.get_child_count()):
        c = o.get_child_at_index(i)
        if c is None:
            continue
        if c.get_role_name() == "label" and c.get_name():
            return c.get_name()
        inner = shown_name(c, depth + 1)
        if inner:
            return inner
    return ""


def find_all(app_name, role, name_re, limit=1, frame_re=None, objects=False):
    Atspi = atspi()
    rx = re.compile(name_re)
    desk = Atspi.get_desktop(0)
    found = []

    def walk(o, depth, gtk4, frame):
        if len(found) >= limit or depth > 40:
            return
        try:
            r = o.get_role_name()
            if r in ("frame", "dialog", "alert", "file chooser", "window"):
                frame = o.get_name()
            if r == role and rx.search(shown_name(o)) and (not frame_re or re.search(frame_re, frame)):
                # Some roots (GTK 4) have no extents; keep looking below.
                try:
                    states = o.get_state_set()
                    if gtk4:
                        e = o.get_extents(Atspi.CoordType.WINDOW)
                        origin = window_origin(frame)
                        ok = origin is not None
                        x, y = (e.x + origin[0], e.y + origin[1]) if ok else (0, 0)
                    else:
                        e = o.get_extents(Atspi.CoordType.SCREEN)
                        ok, x, y = e.x > -10000, e.x, e.y
                    if ok and e.width > 1 and e.height > 1 and states.contains(Atspi.StateType.SHOWING):
                        found.append(o if objects else (x, y, e.width, e.height))
                        return
                except Exception:
                    pass
            for i in range(o.get_child_count()):
                c = o.get_child_at_index(i)
                if c is not None:
                    walk(c, depth + 1, gtk4, frame)
        except Exception:
            pass

    for i in range(desk.get_child_count()):
        app = desk.get_child_at_index(i)
        name = (app.get_name() or "") if app is not None else ""
        if app is not None and app_name.lower() in name.lower():
            walk(app, 0, name in GTK4_APPS, "")
    return found


def find(app_name, role, name_re, frame_re=None, pick=None):
    hits = find_all(app_name, role, name_re, limit=1000 if pick else 1, frame_re=frame_re)
    if pick == "leftmost":
        hits.sort(key=lambda h: (h[0], h[1]))
    elif pick == "topmost":
        hits.sort(key=lambda h: (h[1], h[0]))
    return hits[0] if hits else None


def text_of(app_name, role, name_re, frame_re=None):
    hits = find_all(app_name, role, name_re, frame_re=frame_re, objects=True)
    if not hits:
        return None
    Atspi = atspi()
    t = hits[0].get_text_iface()
    if t is None:
        return hits[0].get_name()
    return Atspi.Text.get_text(t, 0, Atspi.Text.get_character_count(t))


def onboard_window():
    """(x, y, w, h) of the Onboard keyboard window."""
    hit = find("onboard", "frame", "^Onboard$")
    if not hit:
        sys.exit("onboard window not found (is the keyboard shown?)")
    return hit


# --- Onboard key map ------------------------------------------------------------

NAMED = {
    "Return": "RTRN", "Enter": "RTRN", "Tab": "TAB", "Esc": "ESC", "Escape": "ESC",
    "BackSpace": "BKSP", "Delete": "DELE", "Left": "LEFT", "Right": "RGHT",
    "Up": "UP", "Down": "DOWN", "Ctrl": "LCTL", "Alt": "LALT", "Shift": "LFSH",
    "Home": "HOME", "End": "END", "PageUp": "PGUP", "PageDown": "PGDN",
    **{f"F{n}": f"F{n}" for n in range(1, 13)},
}
LAYER_BUTTON = {"numbers": "layer1", "syms": "layer2", "functionkeys": "layer3"}


def load_layout(width, height):
    sys.argv = ["onboard"]
    from Onboard.Config import Config
    config = Config()
    config.init()
    from Onboard import osk
    from Onboard.LayoutLoaderSVG import LayoutLoaderSVG
    from Onboard.utils import Rect
    vk = osk.Virtkey()
    layout = LayoutLoaderSVG().load(vk, config.get_layout_filename(), config.theme_settings.color_scheme_filename)
    # Hide what Onboard hides with the current settings (Keyboard.py does
    # the same when it loads a layout): word suggestions, click helpers.
    suggestions = config.are_word_suggestions_enabled()
    for item in layout.iter_global_items():
        if item.group == "wordlist" or item.id == "wordlistbar":
            item.visible = suggestions
        elif item.group == "nowordlist":
            item.visible = not suggestions
    show_click = config.keyboard.show_click_buttons and not config.lockdown.disable_click_buttons
    for item in layout.iter_items():
        if item.group == "click":
            layout.set_item_visible(item, show_click)
        elif item.group == "noclick":
            layout.set_item_visible(item, not show_click)
    layout.update_log_rect()
    layout.do_fit_inside_canvas(Rect(0, 0, width, height).deflate(config.UNDECORATED_FRAME_WIDTH))
    return layout


def key_map(width, height):
    """{char or key id: (layer, key_id, shifted, (cx, cy))}"""
    layout = load_layout(width, height)
    keys = {}
    chars = {}
    for key in layout.iter_keys():
        if not key.is_visible():
            continue
        layer = key.get_layer()
        r = key.get_canvas_rect()
        centre = (r.x + r.w / 2, r.y + r.h / 2)
        keys.setdefault((layer, key.id), centre)
        labels = key.labels or {}
        for shifted, mask in ((False, 0), (True, 1)):
            label = labels.get(mask)
            if label and len(label) == 1:
                cand = (layer, key.id, shifted, centre)
                old = chars.get(label)
                # Prefer the main layer, then no shift.
                rank = (layer not in (None, "alpha"), shifted)
                if old is None or rank < (old[0] not in (None, "alpha"), old[2]):
                    chars[label] = cand
    return keys, chars


def key_centre(keys, key_id, layer):
    for lay in (layer, None, "alpha"):
        if (lay, key_id) in keys:
            return keys[(lay, key_id)]
    sys.exit(f"key {key_id} not found on layer {layer}")


def osk_type(text):
    wx, wy, ww, wh = onboard_window()
    keys, chars = key_map(ww, wh)
    current = "alpha"

    def press(layer, key_id):
        nonlocal current
        if layer not in (None, current):
            if layer == "alpha":
                tap(*offset(key_centre(keys, "layer0", current)), settle=LAYER_SETTLE)
            else:
                button = LAYER_BUTTON[layer]
                if (None, button) not in keys and (current, button) not in keys:
                    # e.g. "Fn" is only on the numbers layer
                    tap(*offset(key_centre(keys, LAYER_BUTTON["numbers"], current)), settle=LAYER_SETTLE)
                    current = "numbers"
                tap(*offset(key_centre(keys, button, current)), settle=LAYER_SETTLE)
            current = layer
        tap(*offset(key_centre(keys, key_id, current)))
        # Onboard returns to the main layer after one key of another layer.
        if current != "alpha" and key_id not in ("LFSH",):
            current = "alpha"

    def layer_of(key_id):
        layers = [None, current, "alpha"] + sorted({lay for lay, _ in keys if lay}, key=str)
        for lay in layers:
            if (lay, key_id) in keys:
                return lay
        sys.exit(f"key {key_id} is not on the keyboard")

    def offset(p):
        return wx + p[0], wy + p[1]

    for token in re.findall(r"\{[A-Za-z0-9]+\}|.", text, re.S):
        if token.startswith("{") and len(token) > 2:
            name = token[1:-1]
            if name not in NAMED:
                sys.exit(f"unknown key {token}")
            press(layer_of(NAMED[name]), NAMED[name])
            continue
        if token == " ":
            press(None if (None, "SPCE") in keys else "alpha", "SPCE")
            continue
        if token == "\n":
            press(current, "RTRN")
            continue
        if token not in chars:
            sys.exit(f"character {token!r} is not on the keyboard")
        layer, key_id, shifted, _ = chars[token]
        layer = layer or current
        if shifted:
            press(layer, "LFSH")
        press(layer, key_id)


def osk_keys():
    _, _, ww, wh = onboard_window()
    keys, chars = key_map(ww, wh)
    for (layer, kid), c in sorted(keys.items(), key=lambda kv: (str(kv[0][0]), kv[1][1], kv[1][0])):
        print(f"{str(layer):12} {kid:8} {c[0]:6.0f} {c[1]:6.0f}")
    print("".join(sorted(chars)))


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    cmd, rest = sys.argv[1], sys.argv[2:]
    opts, args = {}, []
    while rest:
        a = rest.pop(0)
        if a in ("--frame", "--timeout", "--pick"):
            opts[a[2:]] = rest.pop(0)
        else:
            args.append(a)
    if cmd == "osk-type":
        osk_type(" ".join(args))
    elif cmd == "osk-keys":
        osk_keys()
    elif cmd == "long-press":
        x, y = int(args[0]), int(args[1])
        ms = int(args[2]) if len(args) > 2 else 900
        xdotool("mousemove", x, y)
        time.sleep(0.1)
        xdotool("mousedown", 1)
        time.sleep(ms / 1000)
        xdotool("mouseup", 1)
    elif cmd in ("find", "tap", "wait", "text"):
        app, role, name = args[:3]
        timeout = float(opts.get("timeout", 10 if cmd == "wait" else 0))
        deadline = time.time() + timeout
        while True:
            if cmd == "text":
                hit = text_of(app, role, name, opts.get("frame"))
            else:
                hit = find(app, role, name, opts.get("frame"), opts.get("pick"))
            if hit is not None or time.time() >= deadline:
                break
            time.sleep(0.3)
        if hit is None:
            print(f"not found: {app} {role} {name!r}", file=sys.stderr)
            sys.exit(1)
        if cmd == "text":
            print(hit)
            return
        x, y, w, h = hit
        if cmd == "tap":
            tap(x + w / 2, y + h / 2)
        print(x, y, w, h)
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()

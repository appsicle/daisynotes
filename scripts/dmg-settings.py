# dmgbuild settings for the Muse installer DMG: paper background with the
# rose arrow, Muse.app and /Applications flanking it, 100px icons.
# Used by scripts/package.sh:  dmgbuild -s scripts/dmg-settings.py ...
# Writes the .DS_Store directly — no Finder scripting, works headless in CI.

import os.path

# The app itself is copied in by package.sh AFTER dmgbuild runs (ditto —
# which dmgbuild uses — is denied writing app bundles to mounted images on
# macOS 26). dmgbuild writes the styling: .DS_Store, background, symlink.
background = defines.get("background", "/tmp/muse-dmg-bg.png")  # noqa: F821

volume_name = "Muse"
format = "UDRW"
files = []
symlinks = {"Applications": "/Applications"}

icon_locations = {
    "Muse.app": (150, 185),
    "Applications": (450, 185),
}

window_rect = ((200, 140), (600, 400))
default_view = "icon-view"
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False

icon_size = 100
text_size = 13

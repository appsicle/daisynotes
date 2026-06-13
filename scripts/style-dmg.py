#!/usr/bin/env python3
# Write the installer-window styling (.DS_Store) onto a mounted DMG volume:
# icon view, paper background (.background.png at the volume root), 100px
# icons, DaisyNotes.app and Applications flanking the arrow.
#
# Runs with the dmgbuild pipx venv's python (has ds_store + mac_alias):
#   ~/.local/pipx/venvs/dmgbuild/bin/python scripts/style-dmg.py /Volumes/Daisy\ Notes
#
# Exists because both ditto and cp are denied writing app bundles to mounted
# images on macOS 26, so dmgbuild's own copy step fails; we bake the app in
# via hdiutil -srcfolder and add only the styling here (plain file writes).

import sys

from ds_store import DSStore
from mac_alias import Alias

mount = sys.argv[1].rstrip("/")

icvp = {
    "viewOptionsVersion": 1,
    "backgroundType": 2,
    "backgroundColorRed": 1.0,
    "backgroundColorGreen": 1.0,
    "backgroundColorBlue": 1.0,
    "backgroundImageAlias": Alias.for_file(mount + "/.background.png").to_bytes(),
    "showIconPreview": True,
    "showItemInfo": False,
    "gridOffsetX": 0.0,
    "gridOffsetY": 0.0,
    "gridSpacing": 100.0,
    "arrangeBy": "none",
    "labelOnBottom": True,
    "textSize": 13.0,
    "iconSize": 100.0,
    "scrollPositionX": 0.0,
    "scrollPositionY": 0.0,
}

bwsp = {
    "ShowStatusBar": False,
    "ShowToolbar": False,
    "ShowTabView": False,
    "ShowPathbar": False,
    "ShowSidebar": False,
    "SidebarWidth": 180,
    "WindowBounds": "{{200, 140}, {600, 400}}",
}

with DSStore.open(mount + "/.DS_Store", "w+") as d:
    d["."]["vSrn"] = ("long", 1)
    d["."]["bwsp"] = bwsp
    d["."]["icvp"] = icvp
    d["DaisyNotes.app"]["Iloc"] = (150, 185)
    d["Applications"]["Iloc"] = (450, 185)

print("styled", mount)

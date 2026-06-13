#!/usr/bin/env python3
"""Instance discrete Bold + Bold-Italic faces from the bundled variable fonts.

gpui/font-kit selects a weight by matching among *loaded* font faces and never
applies the variable `wght` axis (verified against gpui 0.2.2). A variable font
loads as a single ~400 face, so bold text (FontWeight 700) has nothing to match
and renders at the regular weight. These pinned wght=700 instances give font-kit
a real Bold face to select, under the same family name as the regular face.

Usage (fontTools required):
    pipx run --spec fonttools python3 scripts/make-bold-fonts.py
Writes assets/fonts/<Family>-Bold.ttf and -BoldItalic.ttf. Re-run and rebuild
after updating the variable source fonts; keep crates/ui/src/fonts.rs in sync.
"""

from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont

FONTS = "assets/fonts/"

# (variable source, output, italic)
JOBS = [
    ("Literata-Variable.ttf", "Literata-Bold.ttf", False),
    ("Literata-Italic-Variable.ttf", "Literata-BoldItalic.ttf", True),
    ("Inter-Variable.ttf", "Inter-Bold.ttf", False),
    ("Inter-Italic-Variable.ttf", "Inter-BoldItalic.ttf", True),
    ("iAWriterQuattro-Variable.ttf", "iAWriterQuattro-Bold.ttf", False),
    ("iAWriterQuattro-Italic-Variable.ttf", "iAWriterQuattro-BoldItalic.ttf", True),
    ("JetBrainsMono-Variable.ttf", "JetBrainsMono-Bold.ttf", False),
    ("JetBrainsMono-Italic-Variable.ttf", "JetBrainsMono-BoldItalic.ttf", True),
]


def base_family(font: TTFont) -> str:
    name = font["name"]
    return (name.getDebugName(16) or name.getDebugName(1)).strip()


def set_name(name, value: str, name_id: int) -> None:
    name.setName(value, name_id, 3, 1, 0x409)  # Windows
    name.setName(value, name_id, 1, 0, 0)  # Mac


def main() -> None:
    for src, out, italic in JOBS:
        font = TTFont(FONTS + src)
        family = base_family(font)
        instantiateVariableFont(font, {"wght": 700}, inplace=True, updateFontNames=False)

        sub = "Bold Italic" if italic else "Bold"
        full = f"{family} {sub}"
        ps = family.replace(" ", "") + "-" + sub.replace(" ", "")

        os2 = font["OS/2"]
        os2.usWeightClass = 700
        fs = os2.fsSelection & ~(1 << 6) | (1 << 5)  # clear REGULAR, set BOLD
        os2.fsSelection = (fs | 1) if italic else (fs & ~1)  # ITALIC bit
        macstyle = font["head"].macStyle | 0x01
        font["head"].macStyle = (macstyle | 0x02) if italic else (macstyle & ~0x02)

        name = font["name"]
        set_name(name, family, 1)
        set_name(name, sub, 2)
        set_name(name, full, 4)
        set_name(name, ps, 6)
        set_name(name, family, 16)
        set_name(name, sub, 17)

        font.save(FONTS + out)
        print(f"{out:34} family={family!r} usWeight=700 italic={italic}")


if __name__ == "__main__":
    main()

//! The bundled fonts: four content font families (all SIL OFL), embedded
//! at compile time as variable TTFs with their italic companions, plus the
//! Sk-Modernist UI chrome face (Regular-only OTF).
//!
//! The app feeds these to `cx.text_system().add_fonts(daisynotes_ui::fonts::all())`
//! before opening the first window, so font-swap layout shift is impossible
//! by construction (PLAN §8).

use std::borrow::Cow;

/// All seventeen bundled font files: Literata, Inter, iA Writer Quattro, and
/// JetBrains Mono — upright + italic variable instances of each, plus discrete
/// Bold + Bold-Italic faces instanced at wght=700 — plus Sk-Modernist Regular,
/// the UI chrome face.
#[must_use]
pub fn all() -> Vec<Cow<'static, [u8]>> {
    vec![
        Cow::Borrowed(include_bytes!("../../../assets/fonts/Literata-Variable.ttf").as_slice()),
        Cow::Borrowed(
            include_bytes!("../../../assets/fonts/Literata-Italic-Variable.ttf").as_slice(),
        ),
        Cow::Borrowed(include_bytes!("../../../assets/fonts/Inter-Variable.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../../../assets/fonts/Inter-Italic-Variable.ttf").as_slice()),
        Cow::Borrowed(
            include_bytes!("../../../assets/fonts/iAWriterQuattro-Variable.ttf").as_slice(),
        ),
        Cow::Borrowed(
            include_bytes!("../../../assets/fonts/iAWriterQuattro-Italic-Variable.ttf").as_slice(),
        ),
        Cow::Borrowed(
            include_bytes!("../../../assets/fonts/JetBrainsMono-Variable.ttf").as_slice(),
        ),
        Cow::Borrowed(
            include_bytes!("../../../assets/fonts/JetBrainsMono-Italic-Variable.ttf").as_slice(),
        ),
        // Discrete Bold + Bold-Italic faces per family, instanced from the
        // variable fonts at wght=700. gpui/font-kit selects a weight by
        // matching among *loaded faces* and never applies the variable wght
        // axis — without these, bold text has no face to match and renders at
        // the regular weight. (Regenerate with scripts/make-bold-fonts.py.)
        Cow::Borrowed(include_bytes!("../../../assets/fonts/Literata-Bold.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../../../assets/fonts/Literata-BoldItalic.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../../../assets/fonts/Inter-Bold.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../../../assets/fonts/Inter-BoldItalic.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../../../assets/fonts/iAWriterQuattro-Bold.ttf").as_slice()),
        Cow::Borrowed(
            include_bytes!("../../../assets/fonts/iAWriterQuattro-BoldItalic.ttf").as_slice(),
        ),
        Cow::Borrowed(include_bytes!("../../../assets/fonts/JetBrainsMono-Bold.ttf").as_slice()),
        Cow::Borrowed(
            include_bytes!("../../../assets/fonts/JetBrainsMono-BoldItalic.ttf").as_slice(),
        ),
        Cow::Borrowed(
            include_bytes!("../../../assets/fonts/Sk-Modernist-Regular.otf").as_slice(),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundles_all_real_font_files() {
        let fonts = all();
        assert_eq!(fonts.len(), 17);
        for font in &fonts {
            // Every file should be a real font, not an empty placeholder:
            // TTF/OTF magic is 0x00010000 ('true' tables) or "OTTO".
            assert!(font.len() > 1024);
            let magic = &font[..4];
            assert!(
                magic == [0x00, 0x01, 0x00, 0x00] || magic == *b"OTTO",
                "unexpected font magic: {magic:?}"
            );
        }
    }
}

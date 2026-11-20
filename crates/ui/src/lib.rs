//! `frayed-ui`: Slint UI layer — шрифт Base 02, регистрация, создание окна.

slint::include_modules!();

pub fn register_fonts() -> Result<(), String> {
    let regular: &[u8] = include_bytes!("../../../assets/fonts/Base-02-Cyrillic.ttf");
    let bold: &[u8] = include_bytes!("../../../assets/fonts/Base-02-Cyrillic-Bold.ttf");

    let mut collection = slint::fontique_012::shared_collection();
    let mut fonts = Vec::new();
    for bytes in [regular, bold] {
        let blob = fontique::Blob::new(std::sync::Arc::new(bytes));
        fonts.extend(collection.register_fonts(blob, None).iter().map(|f| f.0.clone()));
    }
    for script in [*b"Latn", *b"Cyrl"] {
        let key = fontique::FallbackKey::new(fontique::Script::from_bytes(script), None);
        collection.append_fallbacks(key, fonts.iter().cloned());
    }
    Ok(())
}

pub fn base_02_font_family() -> &'static str {
    "Base 02"
}

pub fn create_window() -> crate::MainWindow {
    crate::MainWindow::new().unwrap()
}

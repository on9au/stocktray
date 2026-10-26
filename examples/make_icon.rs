//! Writes `assets/stocktray.ico`, the exe and installer icon, from the tray icon's own renderer:
//! `cargo run --example make_icon`

use std::io::Cursor;

use stocktray::icon::{self, Trend};

const SIZES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 256];

fn main() {
    let font = icon::load_font().expect("Segoe UI Bold or Arial Bold");
    let lines = ["40".to_owned(), "57".to_owned()];
    let pngs: Vec<Vec<u8>> = SIZES
        .iter()
        .map(|&size| {
            let mut png = Vec::new();
            let mut encoder = png::Encoder::new(Cursor::new(&mut png), size, size);
            encoder.set_color(png::ColorType::Rgba);
            let rgba = icon::render(Some(&font), &lines, Trend::Up, size);
            encoder.write_header().unwrap().write_image_data(&rgba).unwrap();
            png
        })
        .collect();

    // ICONDIR, then one ICONDIRENTRY per image, then the PNGs themselves.
    let mut ico = [0u16, 1, SIZES.len() as u16].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>();
    let mut offset = 6 + 16 * SIZES.len();
    for (&size, png) in SIZES.iter().zip(&pngs) {
        let side = if size >= 256 { 0 } else { size as u8 }; // 0 means 256
        ico.extend([side, side, 0, 0]);
        ico.extend(1u16.to_le_bytes()); // colour planes
        ico.extend(32u16.to_le_bytes()); // bits per pixel
        ico.extend((png.len() as u32).to_le_bytes());
        ico.extend((offset as u32).to_le_bytes());
        offset += png.len();
    }
    pngs.iter().for_each(|png| ico.extend(png));

    std::fs::create_dir_all("assets").unwrap();
    std::fs::write("assets/stocktray.ico", ico).unwrap();
    println!("wrote assets/stocktray.ico");
}

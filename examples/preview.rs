//! Renders sample icons at every tray size onto dark and light taskbar backgrounds, magnified,
//! for checking legibility: `cargo run --example preview -- preview.png`

use std::fs::File;
use std::io::BufWriter;

use stocktray::icon::{self, Trend};

const SIZES: [usize; 4] = [16, 20, 24, 32];
const SLOT: usize = 40;
const ZOOM: usize = 6;
const TASKBARS: [[u8; 3]; 2] = [[0x20, 0x20, 0x20], [0xEE, 0xEE, 0xEE]];
const SAMPLES: [(&[&str], Trend); 7] = [
    (&["40", "57"], Trend::Up),
    (&["9", "57"], Trend::Down),
    (&["0", "29"], Trend::Flat),
    (&["1234", "50"], Trend::Up),
    (&["12k"], Trend::Down),
    (&["40", "57"], Trend::Unknown),
    (&["…"], Trend::Unknown),
];

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "preview.png".into());
    let font = icon::load_font();
    let (w, h) = (SAMPLES.len() * TASKBARS.len() * SLOT, SIZES.len() * SLOT);
    let mut canvas: Vec<u8> = (0..w * h).flat_map(|i| TASKBARS[i % w / (w / 2)]).collect();
    for (col, (texts, trend)) in SAMPLES.iter().cycle().take(SAMPLES.len() * TASKBARS.len()).enumerate() {
        for (row, &n) in SIZES.iter().enumerate() {
            let lines: Vec<String> = texts.iter().map(|&t| t.to_owned()).collect();
            let rgba = icon::render(font.as_ref(), &lines, *trend, n as u32);
            let off = (SLOT - n) / 2;
            for (i, px) in rgba.chunks(4).enumerate() {
                let at = ((row * SLOT + off + i / n) * w + col * SLOT + off + i % n) * 3;
                let a = px[3] as f32 / 255.0;
                for c in 0..3 {
                    canvas[at + c] = (px[c] as f32 * a + canvas[at + c] as f32 * (1.0 - a)).round() as u8;
                }
            }
        }
    }

    // Nearest-neighbour zoom so individual pixels are visible.
    let zoomed: Vec<u8> = (0..w * h * ZOOM * ZOOM)
        .flat_map(|i| {
            let (x, y) = (i % (w * ZOOM) / ZOOM, i / (w * ZOOM) / ZOOM);
            canvas[(y * w + x) * 3..][..3].to_vec()
        })
        .collect();
    let mut encoder =
        png::Encoder::new(BufWriter::new(File::create(&out).unwrap()), (w * ZOOM) as u32, (h * ZOOM) as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.write_header().unwrap().write_image_data(&zoomed).unwrap();
    println!("wrote {out}");
}

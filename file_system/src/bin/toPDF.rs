use genpdf::elements::{Image, PageBreak, TableLayout};
use genpdf::{fonts, Alignment, Document, Scale};
use std::error::Error;
use std::path::PathBuf;
use walkdir::WalkDir;

use genpdf::Element as _; // to use .framed() and .padded()

// config
const COLUMNS: usize = 3;
const ROWS: usize = 5;
const IMAGE_SCALE: f64 = 2.0;

fn main() -> Result<(), Box<dyn Error>> {
    // input directory (can be changed or read from args)
    let input_dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| ".".to_string());

    // output directory
    let output_dir = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "qrs.pdf".to_string());

    // load font
    let font_family = fonts::from_files("/usr/share/fonts/truetype/noto", "NotoSans", None)
        .map_err(|e| format!("Error loading font from ./fonts: {e}"))?;

    println!("Generating PDF...");

    let mut doc = Document::new(font_family);
    doc.set_title("QR Codes");

    // page margins
    let mut decorator = genpdf::SimplePageDecorator::new();
    decorator.set_margins(10);
    doc.set_page_decorator(decorator);

    // find and sort PNG files
    let mut images: Vec<PathBuf> = WalkDir::new(&input_dir)
        .min_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            e.path().extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("png"))
                .unwrap_or(false)
        })
        .map(|e| e.into_path())
        .collect();

    images.sort();

    if images.is_empty() {
        eprintln!("No PNG files found in {}", input_dir);
        return Ok(());
    }

    // process images in batches
    let chunks = images.chunks(COLUMNS * ROWS);
    let total_pages = chunks.len();

    for (page_idx, chunk) in chunks.enumerate() {
        let mut table = TableLayout::new(vec![1; COLUMNS]);

        // create grid layout
        for row_idx in 0..ROWS {
            let start = row_idx * COLUMNS;
            if start >= chunk.len() {
                break;
            }

            let mut row = table.row();

            for col_idx in 0..COLUMNS {
                let idx = start + col_idx;
                if idx >= chunk.len() {
                    row.push_element(genpdf::elements::Break::new(0));
                    continue;
                }

                let path = &chunk[idx];

                // load and prepare image
                let img = Image::from_path(path)
                    .map_err(|e| format!("Error loading image {:?}: {e}", path))?
                    .with_alignment(Alignment::Center)
                    .with_scale(Scale::new(IMAGE_SCALE, IMAGE_SCALE));

                // apply border effect
                // I needed to color the background of every page to separate the white of the QR codes from the page background, but I couldn't
                // so this is the best I can do with the time we have
                let img_with_border = img
                    .padded(0.25).framed().padded(0.25).framed()
                    .padded(0.25).framed().padded(0.25).framed()
                    .padded(0.25).framed().padded(0.25).framed()
                    .padded(0.25).framed().padded(0.25).framed()
                    .padded(0.25).framed().padded(0.25).framed()
                    .padded(0.25).framed().padded(0.25).framed()
                    .padded(0.25).framed().padded(0.25).framed()
                    .padded(0.25).framed().padded(0.25).framed()
                    .padded(0.25).framed().padded(0.25).framed()
                    .padded(0.25).framed().padded(0.25).framed()
                    .padded(0.25).framed().padded(0.25).framed()
                    .padded(0.25).framed().padded(0.25).framed()
                    .padded(0.25).framed().padded(0.25).framed()
                    .padded(0.25).framed().padded(0.25).framed()
                    .padded(0.25).framed().padded(0.25).framed()
                    .padded(0.25).framed().padded(0.25).framed()
                    .padded(0.25).framed().padded(0.25).framed()
                    .padded(0.25).framed().padded(0.25).framed()
                    .padded(0.25).framed().padded(0.25).framed()
                    .padded(0.25).framed().padded(0.25).framed();

                row.push_element(img_with_border);
            }

            row.push()?;
        }

        doc.push(table);

        // add page break if not the last page
        if page_idx < total_pages - 1 {
            doc.push(PageBreak::new());
        }
    }

    // save PDF
    let output_file_path = format!("{}/{}", output_dir.trim_end_matches('/'), "QRs.pdf");
    doc.render_to_file(&output_file_path)
        .map_err(|e| format!("Error writing PDF {}: {}", output_file_path, e))?;

    println!("PDF generated with {} images in {}", images.len(), output_file_path);

    Ok(())
}
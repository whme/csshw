use std::path::Path;
use svg_to_ico::svg_to_ico;
extern crate embed_resource;

fn main() {
    svg_to_ico(
        Path::new("res/csshw.svg"),
        96.0,
        Path::new("res/csshw.ico"),
        &[16, 24, 32, 48, 64, 128, 256],
    )
    .expect("Failed to convert SVG to ICO");
    embed_resource::compile_for("res/csshw.rc", ["csshw"], embed_resource::NONE)
        .manifest_required()
        .unwrap();
}

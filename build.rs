extern crate embed_resource;

fn main() {
    embed_resource::compile_for("res/csshw.rc", ["csshw"], embed_resource::NONE);
}

extern crate embed_resource;

fn main() {
    embed_resource::compile_for(
        "res/csshw.rc",
        ["csshw", "csshw-client"],
        embed_resource::NONE,
    );
    embed_resource::compile_for(
        "res/csshw_daemon.rc",
        ["csshw-daemon"],
        embed_resource::NONE,
    );
}

use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;

use packforge_core::{PackOptions, Profile, pack_executable_v2_codec5};

fn main() {
    let mut arguments = env::args().skip(1);
    let input_path = arguments
        .next()
        .expect("usage: m2_codec5_pack INPUT LOADER OUTPUT");
    let loader_path = arguments
        .next()
        .expect("usage: m2_codec5_pack INPUT LOADER OUTPUT");
    let output_path = arguments
        .next()
        .expect("usage: m2_codec5_pack INPUT LOADER OUTPUT");
    assert!(arguments.next().is_none(), "unexpected argument");
    let original = fs::read(&input_path).expect("cannot read input");
    let loader = fs::read(loader_path).expect("cannot read loader");
    let mode = fs::metadata(input_path)
        .expect("cannot stat input")
        .permissions()
        .mode();
    let packed = pack_executable_v2_codec5(
        &original,
        mode,
        PackOptions {
            profile: Profile::Balanced,
            allow_larger: true,
        },
        &loader,
    )
    .expect("codec-5 packing failed");
    fs::write(&output_path, packed.bytes).expect("cannot write output");
    fs::set_permissions(output_path, fs::Permissions::from_mode(0o755))
        .expect("cannot mark output executable");
}

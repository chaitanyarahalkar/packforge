fn main() {
    let apultra = "vendor/apultra";
    let seven_zip = "vendor/7zip";
    let mut build = cc::Build::new();
    build
        .warnings(false)
        .define("NDEBUG", None)
        .include(apultra)
        .include(format!("{apultra}/include"))
        .include(seven_zip)
        .files([
            format!("{apultra}/expand.c"),
            format!("{apultra}/matchfinder.c"),
            format!("{apultra}/shrink.c"),
            format!("{apultra}/lib/divsufsort.c"),
            format!("{apultra}/lib/divsufsort_utils.c"),
            format!("{apultra}/lib/sssort.c"),
            format!("{apultra}/lib/trsort.c"),
            format!("{seven_zip}/Bcj2.c"),
            format!("{seven_zip}/Bcj2Enc.c"),
            "src/bridge.c".to_owned(),
        ])
        .compile("packforge_codec5");
    println!("cargo:rerun-if-changed=src/bridge.c");
    println!("cargo:rerun-if-changed=vendor");
}

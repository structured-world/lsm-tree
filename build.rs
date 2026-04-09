fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_ZSTD");

    if std::env::var("CARGO_FEATURE_ZSTD").is_ok() {
        println!("cargo:rustc-cfg=zstd_any");
    }
}

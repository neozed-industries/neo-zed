fn main() {
    if let Ok(bundled) = std::env::var("NEOZED_BUNDLE") {
        println!("cargo:rustc-env=NEOZED_BUNDLE={}", bundled);
    }
}

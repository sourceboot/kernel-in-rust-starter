// Links every bin with the course's user.ld (one directory up): ET_EXEC at
// 0x1000000, entry _start, the headers in the first PT_LOAD (CONTRACT §A.5, §A.8).
fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-link-arg-bins=-T{dir}/../user.ld");
    println!("cargo:rerun-if-changed=../user.ld");
    println!("cargo:rerun-if-changed=build.rs");
}

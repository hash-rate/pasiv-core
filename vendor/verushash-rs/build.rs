fn main() {
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    let mut build_c = cc::Build::new();
    build_c
        .flag("-fno-lto")
        .include("native/crypto")
        .files(["native/crypto/haraka.c", "native/crypto/haraka_portable.c"]);

    if target_arch == "x86_64" {
        build_c.flag("-maes").flag("-msse4.1").flag("-mpclmul");
    } else if target_arch == "aarch64" {
        build_c.flag("-march=armv8-a+crypto");
    }

    build_c.compile("haraka");

    let mut build_cpp = cc::Build::new();
    build_cpp
        .cpp(true)
        .flag_if_supported("-std=c++17")
        .flag("-fno-lto")
        .include("native/crypto")
        .files([
            "native/crypto/verus_hash.cpp",
            "native/crypto/verus_clhash.cpp",
            "native/crypto/verus_clhash_portable.cpp",
            "native/verushash.cc",
        ]);

    if target_arch == "x86_64" {
        build_cpp.flag("-maes").flag("-msse4.1").flag("-mpclmul");
    } else if target_arch == "aarch64" {
        build_cpp.flag("-march=armv8-a+crypto");
    }

    if target_os == "macos" {
        build_cpp
            .include("/opt/homebrew/include")
            .flag("-mmacosx-version-min=15.5");
    }

    build_cpp.compile("verushash");

    println!("cargo:rustc-link-lib=static=verushash");
    println!("cargo:rustc-link-lib=static=haraka");
    if target_os == "macos" {
        println!("cargo:rustc-link-lib=c++");
    } else {
        println!("cargo:rustc-link-lib=stdc++");
    }
}

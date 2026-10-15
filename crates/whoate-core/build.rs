use std::{env, path::PathBuf, process::Command};

fn run(command: &mut Command) {
    assert!(command.status().expect("run SDK compiler").success());
}

fn main() {
    println!("cargo:rerun-if-changed=src/sample/native.c");
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let object = out.join("capture.o");
    let archive = out.join("libcapture.a");
    // Match the Rust target's actual deployment version; SDK clang defaults can
    // otherwise silently raise the resulting library's minimum macOS version.
    let deployment = Command::new(env::var_os("RUSTC").unwrap())
        .args([
            "--print",
            "deployment-target",
            "--target",
            &env::var("TARGET").unwrap(),
        ])
        .output()
        .expect("query Rust deployment target");
    assert!(deployment.status.success(), "Rust deployment query failed");
    let deployment = String::from_utf8(deployment.stdout).unwrap();
    let version = deployment
        .trim()
        .strip_prefix("MACOSX_DEPLOYMENT_TARGET=")
        .expect("macOS deployment target");
    let architecture = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let architecture = match architecture.as_str() {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        other => panic!("unsupported macOS target architecture: {other}"),
    };
    run(Command::new("xcrun")
        .arg("clang")
        .args(["-arch", architecture])
        .arg(format!("-mmacosx-version-min={version}"))
        .args([
            "-std=c11",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-c",
            "src/sample/native.c",
            "-o",
        ])
        .arg(&object));
    run(Command::new("ar").arg("rcs").arg(&archive).arg(&object));
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=capture");
}

use std::fs::{self, create_dir_all};
use std::path::{Path, PathBuf};

use anyhow::Context as _;

use crate::prelude::*;

const OUTPUT_LIB_NAME: &str = "ironrdp.dll";
const DOTNET_NATIVE_LIB_NAME: &str = "DevolutionsIronRdp.dll";
const DOTNET_NATIVE_LIB_PATH: &str = "dependencies/runtimes/win-x64/native/";
const FFI_RELEASE_PROFILE: &str = "production-ffi";

pub(crate) fn install(sh: &Shell) -> anyhow::Result<()> {
    let _s = Section::new("FFI-INSTALL");

    cargo_install(sh, &DIPLOMAT_TOOL)?;

    Ok(())
}

pub(crate) fn build_dynamic_lib(sh: &Shell, release: bool) -> anyhow::Result<()> {
    let _s = Section::new("BUILD-DYNAMIC-LIBRARY");

    println!("Build IronRDP DLL");

    let mut args = vec!["build", "--package", "ffi"];
    if release {
        args.extend(["--profile", FFI_RELEASE_PROFILE]);
    }
    sh.cmd("cargo").args(&args).run()?;

    let profile_dir = if release { FFI_RELEASE_PROFILE } else { "debug" };

    let root_dir = sh.current_dir();
    let target_dir = cargo_target_dir(&root_dir);
    let profile_dir = target_dir.join(profile_dir);

    let output_lib_path = profile_dir.join(OUTPUT_LIB_NAME);

    let dotnet_native_lib_dir_path: PathBuf = DOTNET_NATIVE_LIB_PATH.parse()?;
    let dotnet_native_lib_path = root_dir.join(&dotnet_native_lib_dir_path).join(DOTNET_NATIVE_LIB_NAME);

    create_dir_all(&dotnet_native_lib_dir_path)
        .with_context(|| format!("failed to create directory {}", dotnet_native_lib_dir_path.display()))?;

    fs::copy(&output_lib_path, &dotnet_native_lib_path).with_context(|| {
        format!(
            "failed to copy {} to {}",
            output_lib_path.display(),
            dotnet_native_lib_path.display()
        )
    })?;

    println!(
        "Copied {} to {}",
        output_lib_path.display(),
        dotnet_native_lib_path.display()
    );

    Ok(())
}

pub(crate) fn build_bindings(sh: &Shell, skip_dotnet_build: bool) -> anyhow::Result<()> {
    let _s = Section::new("BUILD-BINDINGS");

    if !is_installed(sh, "diplomat-tool") {
        anyhow::bail!("`diplomat-tool` binary is missing. Please run `cargo xtask ffi install`.");
    }

    let dotnet_generated_path = "./dotnet/Devolutions.IronRdp/Generated/";
    let temp_dotnet_generated_path = "./dotnet/Devolutions.IronRdp/Generated.tmp/";
    let diplomat_config = "./dotnet-interop-conf.toml";

    // Check if diplomat tool is installed
    sh.change_dir("./ffi");
    let cwd = sh.current_dir();
    let generated_code_dir = cwd.join(dotnet_generated_path);
    if !generated_code_dir.exists() {
        anyhow::bail!("The directory {} does not exist", generated_code_dir.display());
    }
    let temp_generated_code_dir = cwd.join(temp_dotnet_generated_path);
    if temp_generated_code_dir.exists() {
        fs::remove_dir_all(&temp_generated_code_dir)
            .with_context(|| format!("failed to remove temporary directory {}", temp_generated_code_dir.display()))?;
    }
    create_dir_all(&temp_generated_code_dir)
        .with_context(|| format!("failed to create directory {}", temp_generated_code_dir.display()))?;

    sh.cmd("diplomat-tool")
        .arg("dotnet")
        .arg(temp_dotnet_generated_path)
        .arg("-l")
        .arg(diplomat_config)
        .run()?;

    replace_cs_files(&temp_generated_code_dir, &generated_code_dir)?;
    fs::remove_dir_all(&temp_generated_code_dir)
        .with_context(|| format!("failed to remove temporary directory {}", temp_generated_code_dir.display()))?;

    if skip_dotnet_build {
        return Ok(());
    }

    sh.change_dir("./dotnet/Devolutions.IronRdp/");

    cmd!(sh, "dotnet build").run()?;

    Ok(())
}

/// Removes all `.cs` files in the given directory.
fn remove_cs_files(dir: &Path) -> anyhow::Result<()> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("cs") {
                println!("Removing file: {path:?}");
                fs::remove_file(path)?;
            }
        }
    }

    Ok(())
}

fn replace_cs_files(src_dir: &Path, dst_dir: &Path) -> anyhow::Result<()> {
    remove_cs_files(dst_dir)?;

    if src_dir.is_dir() {
        for entry in fs::read_dir(src_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("cs") {
                let destination = dst_dir.join(entry.file_name());
                println!("Moving file: {} -> {}", path.display(), destination.display());
                fs::rename(&path, &destination).with_context(|| {
                    format!("failed to move {} to {}", path.display(), destination.display())
                })?;
            }
        }
    }

    Ok(())
}

fn cargo_target_dir(root_dir: &Path) -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR") {
        Some(dir) => {
            let dir = PathBuf::from(dir);
            if dir.is_absolute() {
                dir
            } else {
                root_dir.join(dir)
            }
        }
        None => root_dir.join("target"),
    }
}

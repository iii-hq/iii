use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ui_dir = manifest_dir.join("src/workers/observability/ui");
    let dist_dir = ui_dir.join("dist");
    let assets = [dist_dir.join("page.js"), dist_dir.join("styles.css")];

    for path in [
        "page.tsx",
        "styles.css",
        "types.ts",
        "build.mjs",
        "package.json",
        "tsconfig.json",
    ] {
        println!("cargo:rerun-if-changed={}", ui_dir.join(path).display());
    }
    println!("cargo:rerun-if-changed=../pnpm-lock.yaml");

    let assets_available = assets.iter().all(|asset| asset.exists());

    if assets_available && assets.iter().all(|asset| is_fresh(asset, &ui_dir)) {
        return;
    }

    if std::env::var_os("SKIP_UI_BUILD").is_some() {
        for asset in &assets {
            if !asset.exists() {
                panic!(
                    "SKIP_UI_BUILD is set but {} is missing; run `pnpm --dir {} build` first",
                    asset.display(),
                    ui_dir.display()
                );
            }
        }
        return;
    }

    let status = match Command::new("pnpm")
        .args(["build"])
        .current_dir(&ui_dir)
        .status()
    {
        Ok(status) => status,
        Err(error) if assets_available => {
            println!(
                "cargo:warning=pnpm is unavailable ({error}); using prebuilt observability UI assets from {}",
                dist_dir.display()
            );
            return;
        }
        Err(error) => panic!("failed to run pnpm in {}: {error}", ui_dir.display()),
    };
    if !status.success() {
        panic!("`pnpm build` failed in {} with {status}", ui_dir.display());
    }

    for asset in &assets {
        if !asset.exists() {
            panic!(
                "pnpm build completed but {} was not generated",
                asset.display()
            );
        }
    }
}

fn is_fresh(asset: &Path, ui_dir: &Path) -> bool {
    let Ok(asset_modified) = asset.metadata().and_then(|meta| meta.modified()) else {
        return false;
    };

    for source in [
        "page.tsx",
        "styles.css",
        "types.ts",
        "build.mjs",
        "package.json",
        "tsconfig.json",
    ] {
        let path = ui_dir.join(source);
        let Ok(modified) = path.metadata().and_then(|meta| meta.modified()) else {
            return false;
        };
        if modified > asset_modified {
            return false;
        }
    }

    true
}

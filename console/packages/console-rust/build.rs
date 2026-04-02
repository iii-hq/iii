use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

fn main() {
    // Only rebuild frontend if REBUILD_FRONTEND is set or in release mode
    let skip_frontend = env::var("SKIP_FRONTEND_BUILD").is_ok();
    let rebuild_frontend = env::var("REBUILD_FRONTEND").is_ok()
        || env::var("PROFILE").map(|p| p == "release").unwrap_or(false);

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let assets_dir = Path::new(&manifest_dir).join("assets");

    // Navigate to workspace root and frontend package
    let packages_dir = Path::new(&manifest_dir).parent().unwrap();
    let workspace_root = packages_dir.parent().unwrap();
    let frontend_dir = packages_dir.join("console-frontend");
    let dist_binary_dir = frontend_dir.join("dist-binary");

    // Check if assets already exist
    let assets_exist = assets_dir.join("index.html").exists();

    if !skip_frontend && (rebuild_frontend || !assets_exist) {
        println!("cargo:warning=Building frontend assets...");

        // Run vite build via pnpm from workspace root
        let build_result = Command::new("pnpm")
            .current_dir(workspace_root)
            .args(["run", "build:binary"])
            .status();

        match build_result {
            Ok(status) if status.success() => {
                println!("cargo:warning=Frontend build completed successfully");
            }
            Ok(status) => {
                println!(
                    "cargo:warning=Frontend build failed with status: {}",
                    status
                );
                if !assets_exist {
                    panic!("Frontend build failed and no existing assets found");
                }
            }
            Err(e) => {
                println!("cargo:warning=Could not run frontend build: {}", e);
                if !assets_exist {
                    panic!("Could not run frontend build and no existing assets found");
                }
            }
        }

        // Copy dist-binary contents to assets
        if dist_binary_dir.exists() {
            // Clean assets directory first to remove stale files
            if assets_dir.exists() {
                fs::remove_dir_all(&assets_dir).expect("Failed to clean assets directory");
            }
            fs::create_dir_all(&assets_dir).expect("Failed to create assets directory");

            // Copy all files from dist-binary to assets
            copy_dir_recursive(&dist_binary_dir, &assets_dir)
                .expect("Failed to copy frontend assets");

            println!("cargo:warning=Assets copied to {:?}", assets_dir);
        }
    } else {
        println!("cargo:warning=Using existing frontend assets");
    }

    // Tell cargo to rerun if source files change
    println!("cargo:rerun-if-changed=../console-frontend/src/");
    println!("cargo:rerun-if-changed=../console-frontend/index.html");
    println!("cargo:rerun-if-changed=../console-frontend/vite.config.ts");
    println!("cargo:rerun-if-env-changed=REBUILD_FRONTEND");
    println!("cargo:rerun-if-env-changed=SKIP_FRONTEND_BUILD");
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let dest_path = dst.join(entry.file_name());

        if path.is_dir() {
            copy_dir_recursive(&path, &dest_path)?;
        } else {
            fs::copy(&path, &dest_path)?;
        }
    }

    Ok(())
}

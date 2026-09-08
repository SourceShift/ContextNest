use std::process::Command;

fn git(args: &[&str]) -> String {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}
fn main() {
    let commit = git(&["rev-parse", "HEAD"]);
    let dirty = !git(&["status", "--porcelain", "--untracked-files=no"]).is_empty();
    println!(
        "cargo:rustc-env=CONTEXTNEST_GIT_COMMIT={}",
        if commit.is_empty() {
            "unknown"
        } else {
            &commit
        }
    );
    println!("cargo:rustc-env=CONTEXTNEST_BUILD_DIRTY={dirty}");
    println!(
        "cargo:rustc-env=CONTEXTNEST_BUILD_PROFILE={}",
        std::env::var("PROFILE").unwrap_or_default()
    );
    // git-path resolves linked-worktree HEAD/index and the common refs directory.
    for path in ["HEAD", "index", "refs", "packed-refs"] {
        let resolved = git(&["rev-parse", "--git-path", path]);
        if !resolved.is_empty() {
            println!("cargo:rerun-if-changed={resolved}");
        }
    }
    for path in git(&["ls-files"]).lines() {
        println!("cargo:rerun-if-changed={path}");
    }
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=build.rs");
}

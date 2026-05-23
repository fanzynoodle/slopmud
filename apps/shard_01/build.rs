use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn rerun_if_git_path(pathspec: &str) {
    if let Some(path) = git(&["rev-parse", "--git-path", pathspec]) {
        if !path.is_empty() && PathBuf::from(&path).exists() {
            println!("cargo:rerun-if-changed={path}");
        }
    }
}

fn main() {
    // Capture build timestamp (UTC) and git metadata for `buildinfo`.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    println!("cargo:rustc-env=SLOPMUD_BUILD_UNIX={now}");

    if let Ok(profile) = std::env::var("PROFILE") {
        println!("cargo:rustc-env=SLOPMUD_PROFILE={profile}");
    }

    // Best-effort human-friendly timestamp (Linux).
    if let Ok(out) = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        && out.status.success()
    {
        let ts = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !ts.is_empty() {
            println!("cargo:rustc-env=SLOPMUD_BUILD_UTC={ts}");
        }
    }

    let sha = env_nonempty("SLOPMUD_GIT_SHA")
        .or_else(|| env_nonempty("GITHUB_SHA"))
        .map(|s| s.chars().take(12).collect::<String>())
        .or_else(|| git(&["rev-parse", "--short", "HEAD"]));
    if let Some(sha) = sha {
        println!("cargo:rustc-env=SLOPMUD_GIT_SHA={sha}");
    }

    // Dirty if `git diff` or `git diff --cached` reports changes.
    let dirty = env_nonempty("SLOPMUD_GIT_DIRTY")
        .map(|v| matches!(v.as_str(), "1" | "true" | "yes"))
        .unwrap_or_else(|| {
            Command::new("git")
                .args(["diff", "--quiet"])
                .status()
                .map(|s| !s.success())
                .unwrap_or(false)
                || Command::new("git")
                    .args(["diff", "--cached", "--quiet"])
                    .status()
                    .map(|s| !s.success())
                    .unwrap_or(false)
        });
    println!(
        "cargo:rustc-env=SLOPMUD_GIT_DIRTY={}",
        if dirty { "1" } else { "0" }
    );

    println!("cargo:rerun-if-env-changed=SLOPMUD_GIT_SHA");
    println!("cargo:rerun-if-env-changed=SLOPMUD_GIT_DIRTY");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");

    // Ensure rebuild when git metadata changes. Use `git --git-path` because
    // worktrees store HEAD/index outside the package directory.
    rerun_if_git_path("HEAD");
    rerun_if_git_path("index");
    rerun_if_git_path("packed-refs");
    if let Some(head_ref) = git(&["symbolic-ref", "-q", "HEAD"]) {
        rerun_if_git_path(&head_ref);
    }

    // Embed all `world/areas/*.yaml` files as compile-time strings.
    //
    // This keeps deploy simple (binary-only) while still letting authors edit YAML.
    // Content changes require a rebuild, but not a hand-edited Rust list.
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let areas_dir = manifest_dir.join("../../world/areas");
    println!("cargo:rerun-if-changed={}", areas_dir.display());

    let mut yamls = Vec::<PathBuf>::new();
    if let Ok(rd) = std::fs::read_dir(&areas_dir) {
        for ent in rd.flatten() {
            let p = ent.path();
            if p.extension().and_then(|s| s.to_str()) == Some("yaml") {
                println!("cargo:rerun-if-changed={}", p.display());
                yamls.push(p);
            }
        }
    }
    yamls.sort();

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let out_path = out_dir.join("world_areas.rs");
    let mut out = String::new();
    out.push_str("pub const WORLD_AREAS_YAML: &[(&str, &str)] = &[\n");
    for p in yamls {
        let name = p
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown.yaml");
        let abs = p.to_string_lossy();
        out.push_str(&format!("  (r#\"{name}\"#, include_str!(r#\"{abs}\"#)),\n"));
    }
    out.push_str("];\n");
    std::fs::write(out_path, out).expect("write world_areas.rs");
}

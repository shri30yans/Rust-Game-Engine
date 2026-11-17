//! Reproduces the sample arena with real Blender and compares its committed artifacts.

use std::error::Error;
use std::fs;
use std::io;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn sample_arena_is_reproducible_from_blender() -> Result<(), Box<dyn Error>> {
    let Some(blender) = blender_executable() else {
        return Ok(());
    };
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let test_root = TestDirectory::create(
        repository
            .join("target")
            .join(format!("cook-blender-repro-{}", std::process::id())),
    )?;
    let first = test_root.path().join("first");
    let second = test_root.path().join("second");

    run_sample_arena(&blender, &repository, &first)?;
    run_sample_arena(&blender, &repository, &second)?;

    assert_same_directory(
        "the first and second generated source directories",
        &first.join("source"),
        &second.join("source"),
    )?;
    assert_same_directory(
        "the first and second generated Sandbox cooked directories",
        &first.join("sandbox/content/cooked"),
        &second.join("sandbox/content/cooked"),
    )?;
    assert_same_directory(
        "the first and second generated IMAGINE cooked directories",
        &first.join("imagine/content/cooked"),
        &second.join("imagine/content/cooked"),
    )?;

    assert_same_directory(
        "the generated source directory and committed Sandbox source",
        &first.join("source"),
        &repository.join("games/sandbox/content/source"),
    )?;
    assert_same_arena_cooked_outputs(
        "the generated Sandbox cooked outputs and committed Sandbox cooked outputs",
        &first.join("sandbox/content/cooked"),
        &repository.join("games/sandbox/content/cooked"),
    )?;
    assert_same_arena_cooked_outputs(
        "the generated IMAGINE cooked outputs and committed IMAGINE cooked outputs",
        &first.join("imagine/content/cooked"),
        &repository.join("games/imagine/content/cooked"),
    )?;
    Ok(())
}

fn blender_executable() -> Option<PathBuf> {
    let Some(blender) = std::env::var_os("BLENDER")
        .map(PathBuf::from)
        .or_else(default_blender_path)
    else {
        eprintln!(
            "cargo test: skipped sample_arena_is_reproducible_from_blender; \
             BLENDER is not set and this platform has no default Blender path"
        );
        return None;
    };
    if executable_file(&blender) {
        Some(blender)
    } else {
        eprintln!(
            "cargo test: skipped sample_arena_is_reproducible_from_blender; \
             BLENDER is not an executable file: {}",
            blender.display()
        );
        None
    }
}

#[cfg(target_os = "macos")]
fn default_blender_path() -> Option<PathBuf> {
    Some(PathBuf::from(
        "/Applications/Blender.app/Contents/MacOS/Blender",
    ))
}

#[cfg(not(target_os = "macos"))]
fn default_blender_path() -> Option<PathBuf> {
    None
}

#[cfg(unix)]
fn executable_file(path: &Path) -> bool {
    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable_file(path: &Path) -> bool {
    path.is_file()
}

fn run_sample_arena(blender: &Path, repository: &Path, output: &Path) -> io::Result<()> {
    let status = Command::new(blender)
        .current_dir(repository)
        .args([
            "--background",
            "--python",
            "tools/blender/make_sample_arena.py",
            "--",
        ])
        .arg("--blend")
        .arg(output.join("blender/sample_arena.blend"))
        .arg("--source")
        .arg(output.join("source"))
        .arg("--sandbox-content")
        .arg(output.join("sandbox/content"))
        .arg("--imagine-content")
        .arg(output.join("imagine/content"))
        .status()
        .map_err(|error| operation_error("start Blender sample-arena generation", output, error))?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "Blender sample-arena generation, export, and cook failed for {} with {status}",
            output.display()
        )))
    }
}

fn assert_same_directory(description: &str, left: &Path, right: &Path) -> io::Result<()> {
    let left_bytes = directory_bytes(left)?;
    let right_bytes = directory_bytes(right)?;
    if left_bytes == right_bytes {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "byte mismatch between {description}: {} and {}",
            left.display(),
            right.display()
        )))
    }
}

fn assert_same_arena_cooked_outputs(
    description: &str,
    left: &Path,
    right: &Path,
) -> io::Result<()> {
    let left_bytes = arena_cooked_bytes(left)?;
    let right_bytes = arena_cooked_bytes(right)?;
    if left_bytes == right_bytes {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "byte mismatch between {description}: {} and {}",
            left.display(),
            right.display()
        )))
    }
}

type DirectoryBytes = Vec<(PathBuf, Vec<u8>)>;

fn directory_bytes(directory: &Path) -> io::Result<DirectoryBytes> {
    let mut bytes = Vec::new();
    collect_directory_bytes(directory, directory, &mut bytes)?;
    bytes.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(bytes)
}

fn arena_cooked_bytes(directory: &Path) -> io::Result<DirectoryBytes> {
    let mut bytes = Vec::new();
    for name in [
        "index.ron",
        "sample_arena.level.ron",
        "sample_arena.meshpack",
    ] {
        collect_file_bytes(directory, &directory.join(name), &mut bytes)?;
    }
    for name in ["prefabs", "textures"] {
        collect_directory_bytes(directory, &directory.join(name), &mut bytes)?;
    }
    bytes.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(bytes)
}

fn collect_directory_bytes(
    root: &Path,
    directory: &Path,
    bytes: &mut DirectoryBytes,
) -> io::Result<()> {
    for entry in fs::read_dir(directory)
        .map_err(|error| operation_error("read generated output directory", directory, error))?
    {
        let entry = entry
            .map_err(|error| operation_error("read generated output entry", directory, error))?;
        let path = entry.path();
        if path.is_dir() {
            collect_directory_bytes(root, &path, bytes)?;
        } else {
            collect_file_bytes(root, &path, bytes)?;
        }
    }
    Ok(())
}

fn collect_file_bytes(root: &Path, path: &Path, bytes: &mut DirectoryBytes) -> io::Result<()> {
    let relative = path.strip_prefix(root).map_err(|error| {
        io::Error::other(format!(
            "make {} relative to {}: {error}",
            path.display(),
            root.display()
        ))
    })?;
    let contents = fs::read(path)
        .map_err(|error| operation_error("read generated output file", path, error))?;
    bytes.push((relative.to_path_buf(), contents));
    Ok(())
}

fn operation_error(operation: &str, path: &Path, error: io::Error) -> io::Error {
    io::Error::new(
        error.kind(),
        format!("failed to {operation} at {}: {error}", path.display()),
    )
}

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn create(path: PathBuf) -> io::Result<Self> {
        if path.try_exists()? {
            fs::remove_dir_all(&path)
                .map_err(|error| operation_error("remove prior test directory", &path, error))?;
        }
        fs::create_dir_all(&path)
            .map_err(|error| operation_error("create test directory", &path, error))?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.path) {
            eprintln!(
                "cargo test: could not clean Blender reproducibility directory {}: {error}",
                self.path.display()
            );
        }
    }
}

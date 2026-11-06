//! Finding a game's executable and starting it.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use foyer_patch::Manifest;
use foyer_types::Platform;

/// The platform this client runs on, as builds are labelled.
pub fn current_platform() -> Platform {
    if cfg!(target_os = "windows") {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::Macos
    } else {
        Platform::Linux
    }
}

/// Names that ship next to games but are not the game.
const NOT_THE_GAME: &[&str] = &[
    "unins",
    "uninstall",
    "crashhandler",
    "crashreport",
    "vc_redist",
    "vcredist",
    "dxsetup",
    "dxwebsetup",
    "directx",
    "dotnet",
    "oalinst",
    "physx",
    "ue4prereq",
    "ueprereq",
    "setup",
    "installer",
    "launcherhelper",
    "notification_helper",
];

/// Manifest paths that could start the game on `platform`, best guess first.
///
/// `hint` is the game's slug or title; a file named like it ranks higher.
/// `is_executable` tells whether an installed file has the executable bit
/// (only consulted on Linux and macOS).
pub fn executable_candidates(
    manifest: &Manifest,
    platform: Platform,
    hint: &str,
    is_executable: impl Fn(&str) -> bool,
) -> Vec<String> {
    let hint = normalize(hint);
    let mut candidates: Vec<(i64, String)> = manifest
        .files
        .keys()
        .filter_map(|path| {
            let lower = path.to_lowercase();
            let name = lower.rsplit('/').next().unwrap_or(&lower);
            let stem = name.split('.').next().unwrap_or(name);
            let fits = match platform {
                Platform::Windows => name.ends_with(".exe") || name.ends_with(".bat"),
                Platform::Macos => {
                    lower.contains(".app/contents/macos/")
                        || name.ends_with(".command")
                        || (!name.contains('.') && is_executable(path))
                }
                Platform::Linux => {
                    name.ends_with(".x86_64")
                        || name.ends_with(".appimage")
                        || name.ends_with(".sh")
                        || (!name.contains(".so") && is_executable(path))
                }
                Platform::Web => name == "index.html",
            };
            if !fits || NOT_THE_GAME.iter().any(|bad| stem.contains(bad)) {
                return None;
            }
            let depth = path.matches('/').count() as i64;
            let mut score = -depth * 10;
            let norm = normalize(stem);
            if !hint.is_empty() && norm == hint {
                score += 50;
            } else if !hint.is_empty() && (norm.contains(&hint) || hint.contains(&norm)) {
                score += 25;
            }
            if name.ends_with(".bat") || name.ends_with(".sh") {
                score -= 5;
            }
            Some((score, path.clone()))
        })
        .collect();
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    candidates.into_iter().map(|(_, path)| path).collect()
}

fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Whether the file at `path` has an executable bit.
pub fn has_exec_bit(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}

/// Add the executable bits (no-op on Windows).
pub fn make_executable(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(perms.mode() | 0o755);
        std::fs::set_permissions(path, perms)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

/// The absolute path of a manifest path inside an install.
pub fn resolve(install_dir: &Path, manifest_path: &str) -> PathBuf {
    let mut out = install_dir.to_path_buf();
    out.extend(manifest_path.split('/'));
    out
}

/// Start the game. The working directory is the executable's folder, which
/// is what most engines expect.
pub fn launch(install_dir: &Path, executable: &str, args: &[String]) -> io::Result<Child> {
    let exe = resolve(install_dir, executable);
    if !exe.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{} is missing; verify the game files", exe.display()),
        ));
    }
    let workdir = exe.parent().unwrap_or(install_dir).to_path_buf();
    let lower = executable.to_lowercase();

    let mut command = if lower.ends_with(".sh") {
        let mut c = Command::new("sh");
        c.arg(&exe);
        c
    } else if lower.ends_with(".bat") {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(&exe);
        c
    } else {
        // Builds made before the manifest kept the executable bit may still
        // lack it.
        if !has_exec_bit(&exe) {
            let _ = make_executable(&exe);
        }
        Command::new(&exe)
    };
    command.args(args).current_dir(workdir);
    command.spawn()
}

#[cfg(test)]
mod tests {
    use super::*;
    use foyer_patch::FileEntry;

    fn manifest(paths: &[&str]) -> Manifest {
        Manifest {
            chunker: Default::default(),
            files: paths
                .iter()
                .map(|p| {
                    (
                        p.to_string(),
                        FileEntry {
                            size: 0,
                            hash: "0".repeat(64),
                            chunks: Vec::new(),
                        },
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn picks_the_game_exe_on_windows() {
        let m = manifest(&[
            "Moss Garden.exe",
            "UnityCrashHandler64.exe",
            "unins000.exe",
            "redist/vc_redist.x64.exe",
            "MossGarden_Data/level0",
            "tools/editor.exe",
        ]);
        let found = executable_candidates(&m, Platform::Windows, "moss-garden", |_| false);
        assert_eq!(found, ["Moss Garden.exe", "tools/editor.exe"]);
    }

    #[test]
    fn uses_exec_bits_and_known_suffixes_on_linux() {
        let m = manifest(&[
            "lib/libfoo.so.1",
            "bin/game",
            "Game.x86_64",
            "readme.txt",
            "start.sh",
        ]);
        let found = executable_candidates(&m, Platform::Linux, "game", |p| {
            p == "bin/game" || p == "lib/libfoo.so.1"
        });
        // Named like the game beats a shallower launcher script.
        assert_eq!(found, ["Game.x86_64", "bin/game", "start.sh"]);
    }
}

//! Desktop integration: a right-click / "Open with" entry for XISF and FITS
//! files that launches this GUI with the selected files.
//!
//! * Windows: per-extension verbs under
//!   `HKCU\Software\Classes\SystemFileAssociations\<ext>\shell\xisf2png`.
//!   Current user only, no admin rights needed. Explorer starts one process
//!   per selected file; `instance.rs` merges them into one window.
//! * Linux: a `.desktop` entry in `~/.local/share/applications` (with `%F`,
//!   so file managers pass the whole selection at once) plus a MIME type for
//!   `.xisf`, which is not in the shared MIME database.
//! * macOS: nothing to install; the window accepts dropped files.

#[cfg(not(target_os = "macos"))]
use std::path::PathBuf;

pub const EXTENSIONS: &[&str] = &["xisf", "fits", "fit", "fts"];

pub struct Outcome {
    pub ok: bool,
    pub message: String,
}

#[cfg(not(target_os = "macos"))]
fn exe_path() -> Result<PathBuf, String> {
    // When running from a Linux AppImage, `current_exe()` points inside the
    // ephemeral `/tmp/.mount_*` directory that disappears on exit; use the
    // real AppImage path instead.
    #[cfg(all(unix, not(target_os = "macos")))]
    if let Some(appimage) = std::env::var_os("APPIMAGE").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(appimage));
    }
    std::env::current_exe().map_err(|e| format!("cannot determine this program's path: {e}"))
}

/// Format an executable path for a FreeDesktop `.desktop` `Exec=` key inside
/// double quotes, escaping both general string-value backslashes and Exec
/// special characters (`"`, `` ` ``, `$`, `\`, and `%` -> `%%`).
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
fn format_desktop_exec_path(exe: &std::path::Path) -> Result<String, String> {
    let raw = exe
        .to_str()
        .ok_or_else(|| "executable path is not valid UTF-8".to_string())?;
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '\n' | '\r' | '\0' => {
                return Err("executable path contains control characters".into());
            }
            '\\' => out.push_str(r"\\\\"),
            '"' => out.push_str(r#"\\""#),
            '`' => out.push_str(r"\\`"),
            '$' => out.push_str(r"\\$"),
            '%' => out.push_str("%%"),
            c => out.push(c),
        }
    }
    Ok(out)
}

#[cfg(any(test, all(unix, not(target_os = "macos"))))]
fn desktop_entry(exe: &std::path::Path) -> Result<String, String> {
    let exec = format_desktop_exec_path(exe)?;
    Ok(format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=xisf2png\n\
         Comment=Convert XISF / FITS astrophotos to PNG\n\
         Exec=\"{exec}\" %F\n\
         Icon=xisf2png\n\
         Terminal=false\n\
         Categories=Graphics;Photography;\n\
         MimeType=image/fits;application/fits;image/x-fits;application/x-xisf;\n"
    ))
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------
#[cfg(windows)]
mod imp {
    use super::{exe_path, Outcome, EXTENSIONS};
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
    use winreg::RegKey;

    const VERB: &str = "xisf2png";
    const LABEL: &str = "Convert to PNG with xisf2png";

    // Registry access goes through the Win32 API (winreg crate). An earlier
    // version spawned hidden `reg.exe` processes, which Windows Defender's
    // behavioural heuristics flagged as "DefenseEvasion" — a fair reading of
    // that pattern, so we do not do that anymore.

    fn key_path(ext: &str) -> String {
        format!(r"Software\Classes\SystemFileAssociations\.{ext}\shell\{VERB}")
    }

    pub fn is_installed() -> bool {
        let Ok(exe) = exe_path() else {
            return false;
        };
        let expected = format!("\"{}\" \"%1\"", exe.display());
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        EXTENSIONS.iter().all(|ext| {
            hkcu.open_subkey_with_flags(format!(r"{}\command", key_path(ext)), KEY_READ)
                .and_then(|k| k.get_value::<String, _>(""))
                .is_ok_and(|cmd| cmd == expected)
        })
    }

    pub fn install() -> Outcome {
        let exe = match exe_path() {
            Ok(p) => p.display().to_string(),
            Err(e) => {
                return Outcome {
                    ok: false,
                    message: e,
                }
            }
        };
        let command = format!("\"{exe}\" \"%1\"");
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        for ext in EXTENSIONS {
            let result = (|| -> std::io::Result<()> {
                let (verb, _) = hkcu.create_subkey(key_path(ext))?;
                verb.set_value("", &LABEL)?;
                verb.set_value("Icon", &exe)?;
                // Player: Explorer invokes the verb for any number of selected
                // items (the default "Document" model stops at 15).
                verb.set_value("MultiSelectModel", &"Player")?;
                let (cmd, _) = verb.create_subkey("command")?;
                cmd.set_value("", &command)?;
                Ok(())
            })();
            if let Err(e) = result {
                return Outcome {
                    ok: false,
                    message: format!("registry write failed for .{ext}: {e}"),
                };
            }
        }
        Outcome {
            ok: true,
            message: format!(
                "Added \"{LABEL}\" to the right-click menu of .{} files (current user).",
                EXTENSIONS.join(", .")
            ),
        }
    }

    pub fn uninstall() -> Outcome {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        for ext in EXTENSIONS {
            // Ignore "not found" so a partial install can still be cleaned up.
            let _ = hkcu.delete_subkey_all(key_path(ext));
        }
        Outcome {
            ok: true,
            message: "Removed the right-click menu entry.".into(),
        }
    }

    pub const INSTALL_LABEL: &str = "Add to Explorer right-click menu";
    pub const UNINSTALL_LABEL: &str = "Remove from Explorer right-click menu";
    pub const HINT: &str = "Right-click one or more .xisf/.fits files in Explorer → \"Convert to PNG with xisf2png\". \
                            You can also drop files or a folder onto this window.";
}

// ---------------------------------------------------------------------------
// Linux (and other Unix with XDG desktops)
// ---------------------------------------------------------------------------
#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    use super::{desktop_entry, exe_path, Outcome};
    use std::path::{Path, PathBuf};
    use std::process::Command;

    fn data_home() -> Result<PathBuf, String> {
        if let Some(p) = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
        {
            return Ok(p);
        }
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .ok_or_else(|| "neither XDG_DATA_HOME nor HOME is set".to_string())?;
        Ok(home.join(".local/share"))
    }
    fn desktop_file(home: &Path) -> PathBuf {
        home.join("applications/xisf2png.desktop")
    }
    fn mime_file(home: &Path) -> PathBuf {
        home.join("mime/packages/xisf2png.xml")
    }
    fn icon_file(home: &Path) -> PathBuf {
        home.join("icons/hicolor/256x256/apps/xisf2png.png")
    }

    pub fn is_installed() -> bool {
        let (Ok(home), Ok(exe)) = (data_home(), exe_path()) else {
            return false;
        };
        let Ok(expected) = desktop_entry(&exe) else {
            return false;
        };
        std::fs::read_to_string(desktop_file(&home)).is_ok_and(|actual| actual == expected)
    }

    fn write(path: &Path, contents: &[u8]) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(path, contents).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn install() -> Outcome {
        let home = match data_home() {
            Ok(h) => h,
            Err(e) => {
                return Outcome {
                    ok: false,
                    message: e,
                }
            }
        };
        let exe = match exe_path() {
            Ok(p) => p,
            Err(e) => {
                return Outcome {
                    ok: false,
                    message: e,
                }
            }
        };
        let desktop = match desktop_entry(&exe) {
            Ok(d) => d,
            Err(e) => {
                return Outcome {
                    ok: false,
                    message: e,
                }
            }
        };
        let mime = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\">\n\
             \x20 <mime-type type=\"application/x-xisf\">\n\
             \x20   <comment>XISF image (PixInsight)</comment>\n\
             \x20   <glob pattern=\"*.xisf\"/>\n\
             \x20   <magic priority=\"60\"><match type=\"string\" offset=\"0\" value=\"XISF0100\"/></magic>\n\
             \x20 </mime-type>\n\
             </mime-info>\n";

        let desk_path = desktop_file(&home);
        let steps = [
            write(&desk_path, desktop.as_bytes()),
            write(&mime_file(&home), mime.as_bytes()),
            write(
                &icon_file(&home),
                include_bytes!("../../../assets/icon-256.png"),
            ),
        ];
        if let Some(Err(e)) = steps.into_iter().find(|r| r.is_err()) {
            return Outcome {
                ok: false,
                message: e,
            };
        }
        // Best effort: refresh the desktop databases if the tools exist.
        let _ = Command::new("update-desktop-database")
            .arg(home.join("applications"))
            .output();
        let _ = Command::new("update-mime-database")
            .arg(home.join("mime"))
            .output();
        Outcome {
            ok: true,
            message: format!(
                "Installed {} — xisf2png now appears under \"Open with\" for FITS and XISF files.",
                desk_path.display()
            ),
        }
    }

    pub fn uninstall() -> Outcome {
        let home = match data_home() {
            Ok(h) => h,
            Err(e) => {
                return Outcome {
                    ok: false,
                    message: e,
                }
            }
        };
        for p in [desktop_file(&home), mime_file(&home), icon_file(&home)] {
            let _ = std::fs::remove_file(p);
        }
        let _ = Command::new("update-desktop-database")
            .arg(home.join("applications"))
            .output();
        let _ = Command::new("update-mime-database")
            .arg(home.join("mime"))
            .output();
        Outcome {
            ok: true,
            message: "Removed the desktop entry.".into(),
        }
    }

    pub const INSTALL_LABEL: &str = "Install \"Open with xisf2png\" desktop entry";
    pub const UNINSTALL_LABEL: &str = "Remove desktop entry";
    pub const HINT: &str = "Select .xisf/.fits files in your file manager → Open with → xisf2png. \
                            You can also drop files or a folder onto this window.";
}

// ---------------------------------------------------------------------------
// macOS: no installer; the .app accepts drops on the window.
// ---------------------------------------------------------------------------
#[cfg(target_os = "macos")]
mod imp {
    use super::Outcome;

    pub fn is_installed() -> bool {
        false
    }
    pub fn install() -> Outcome {
        Outcome {
            ok: false,
            message: "Not needed on macOS: drop files onto this window, or run \
                      `open -a xisf2png --args <files>`."
                .into(),
        }
    }
    pub fn uninstall() -> Outcome {
        install()
    }
    pub const INSTALL_LABEL: &str = "";
    pub const UNINSTALL_LABEL: &str = "";
    pub const HINT: &str = "Drop .xisf/.fits files or a folder onto this window. \
                            From Terminal: open -a xisf2png --args <files>";
}

pub use imp::{install, is_installed, uninstall, HINT, INSTALL_LABEL, UNINSTALL_LABEL};

/// Whether this platform has something to install.
pub fn supported() -> bool {
    !INSTALL_LABEL.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn desktop_exec_escapes_special_chars() {
        let path = Path::new(r#"/opt/100% $user "app" `bin`\xisf2png-gui"#);
        let entry = desktop_entry(path).unwrap();
        assert!(
            entry.contains(r#"Exec="/opt/100%% \\$user \\"app\\" \\`bin\\`\\\\xisf2png-gui" %F"#),
            "unexpected entry:\n{entry}"
        );
    }

    #[test]
    fn desktop_exec_rejects_newlines() {
        let path = Path::new("/opt/bad\npath/xisf2png-gui");
        assert!(desktop_entry(path).is_err());
    }
}

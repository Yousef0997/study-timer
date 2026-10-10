//! "Start when I log in": a desktop entry in ~/.config/autostart that GNOME runs at login.

use std::fs;
use std::io;
use std::path::PathBuf;

use crate::model::home_dir;

fn entry_path() -> PathBuf {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home_dir().join(".config"));
    config.join("autostart").join("study-timer.desktop")
}

pub fn is_enabled() -> bool {
    entry_path().exists()
}

pub fn set_enabled(enabled: bool) -> io::Result<()> {
    let path = entry_path();
    if !enabled {
        return match fs::remove_file(&path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let exe = std::env::current_exe()?;
    let exe = exe.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    fs::create_dir_all(path.parent().expect("autostart dir"))?;
    fs::write(
        &path,
        format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Study Timer\n\
             Comment=Start your study schedule at login\n\
             Exec=\"{exe}\" --background\n\
             Icon=study-timer\n\
             Terminal=false\n\
             X-GNOME-Autostart-enabled=true\n\
             X-GNOME-Autostart-Delay=5\n"
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_creates_and_removes_login_entry() {
        let dir = std::env::temp_dir().join(format!("study-timer-autostart-{}", std::process::id()));
        // SAFETY: this is the only test that touches the environment.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &dir) };
        assert!(!is_enabled());
        set_enabled(true).unwrap();
        assert!(is_enabled());
        let entry = fs::read_to_string(dir.join("autostart/study-timer.desktop")).unwrap();
        assert!(entry.contains("--background"));
        set_enabled(false).unwrap();
        assert!(!is_enabled());
        set_enabled(false).unwrap();
        let _ = fs::remove_dir_all(dir);
    }
}

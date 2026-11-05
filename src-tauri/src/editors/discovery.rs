use super::catalog::EDITORS;
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub enum Launcher {
    Command(PathBuf),
    MacApp(PathBuf),
    Desktop { gio: PathBuf, entry: PathBuf },
}

#[derive(Clone, serde::Serialize)]
pub struct InstalledEditor {
    pub id: String,
    pub name: String,
    #[serde(skip)]
    pub launcher: Launcher,
}

pub struct Locations {
    pub binaries: Vec<PathBuf>,
    pub applications: Vec<PathBuf>,
    pub desktops: Vec<PathBuf>,
}
impl Locations {
    pub fn system() -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let mut binaries: Vec<_> = std::env::var_os("PATH")
            .map(|p| {
                std::env::split_paths(&p)
                    .filter(|p| p.is_absolute())
                    .collect()
            })
            .unwrap_or_default();
        binaries.extend(
            [
                "/opt/homebrew/bin",
                "/usr/local/bin",
                "/usr/bin",
                "/snap/bin",
            ]
            .map(PathBuf::from),
        );
        let mut applications = Vec::new();
        let mut desktops = Vec::new();
        if let Some(home) = &home {
            binaries.extend(
                [".local/bin", ".local/share/JetBrains/Toolbox/scripts"].map(|p| home.join(p)),
            );
            if cfg!(target_os = "macos") {
                applications.push(home.join("Applications"));
            }
            if cfg!(target_os = "linux") {
                let data = std::env::var_os("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .unwrap_or_else(|| home.join(".local/share"));
                desktops.push(data.join("applications"));
                desktops.push(home.join(".local/share/flatpak/exports/share/applications"));
            }
        }
        if cfg!(target_os = "macos") {
            applications.push("/Applications".into());
        }
        if cfg!(target_os = "linux") {
            let data = std::env::var_os("XDG_DATA_DIRS")
                .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
            desktops.extend(
                std::env::split_paths(&data)
                    .filter(|p| p.is_absolute())
                    .map(|p| p.join("applications")),
            );
            desktops.extend(
                [
                    "/var/lib/flatpak/exports/share/applications",
                    "/var/lib/snapd/desktop/applications",
                ]
                .map(PathBuf::from),
            );
        }
        Self {
            binaries,
            applications,
            desktops,
        }
    }
    fn executable(&self, name: &str) -> Option<PathBuf> {
        self.binaries
            .iter()
            .map(|p| p.join(name))
            .find(|p| executable(p))
    }
}
fn executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

pub fn discover(locations: &Locations) -> Vec<InstalledEditor> {
    let gio = locations.executable("gio");
    EDITORS
        .iter()
        .filter_map(|editor| {
            let app = locations
                .applications
                .iter()
                .flat_map(|base| editor.apps.iter().map(move |name| base.join(name)))
                .find(|p| p.join("Contents/Info.plist").is_file());
            let launcher = if let Some(app) = app {
                Some(Launcher::MacApp(app))
            } else if let Some(command) = editor
                .commands
                .iter()
                .find_map(|name| locations.executable(name))
            {
                Some(Launcher::Command(command))
            } else {
                gio.as_ref().and_then(|gio| {
                    locations
                        .desktops
                        .iter()
                        .flat_map(|base| editor.desktops.iter().map(move |name| base.join(name)))
                        .find(|p| {
                            std::fs::read_to_string(p).is_ok_and(|s| {
                                s.lines().any(|l| l.trim() == "Type=Application")
                                    && !s.lines().any(|l| l.trim() == "Hidden=true")
                            })
                        })
                        .map(|entry| Launcher::Desktop {
                            gio: gio.clone(),
                            entry,
                        })
                })
            }?;
            Some(InstalledEditor {
                id: editor.id.into(),
                name: editor.name.into(),
                launcher,
            })
        })
        .collect()
}

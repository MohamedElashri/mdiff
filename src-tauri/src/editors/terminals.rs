//! Fixed launch strategies: folder paths are separate argv values, never shell source.
use super::discovery::{InstalledEditor, Launcher, Locations};
use std::{path::Path, process::Stdio};
use tokio::process::Command;
struct Spec {
    id: &'static str,
    name: &'static str,
    binary: &'static str,
    app: &'static str,
    mac_only: bool,
}
const TERMINALS: &[Spec] = &[
    Spec {
        id: "terminal",
        name: "Terminal",
        binary: "",
        app: "Terminal.app",
        mac_only: true,
    },
    Spec {
        id: "iterm",
        name: "iTerm",
        binary: "",
        app: "iTerm.app",
        mac_only: true,
    },
    Spec {
        id: "ghostty",
        name: "Ghostty",
        binary: "ghostty",
        app: "Ghostty.app",
        mac_only: false,
    },
    Spec {
        id: "kitty",
        name: "kitty",
        binary: "kitty",
        app: "kitty.app",
        mac_only: false,
    },
    Spec {
        id: "alacritty",
        name: "Alacritty",
        binary: "alacritty",
        app: "Alacritty.app",
        mac_only: false,
    },
    Spec {
        id: "wezterm",
        name: "WezTerm",
        binary: "wezterm",
        app: "WezTerm.app",
        mac_only: false,
    },
    Spec {
        id: "gnome-terminal",
        name: "GNOME Terminal",
        binary: "gnome-terminal",
        app: "",
        mac_only: false,
    },
    Spec {
        id: "kgx",
        name: "GNOME Console",
        binary: "kgx",
        app: "",
        mac_only: false,
    },
    Spec {
        id: "konsole",
        name: "Konsole",
        binary: "konsole",
        app: "",
        mac_only: false,
    },
    Spec {
        id: "xfce4-terminal",
        name: "Xfce Terminal",
        binary: "xfce4-terminal",
        app: "",
        mac_only: false,
    },
    Spec {
        id: "foot",
        name: "foot",
        binary: "foot",
        app: "",
        mac_only: false,
    },
    Spec {
        id: "xterm",
        name: "xterm",
        binary: "xterm",
        app: "",
        mac_only: false,
    },
];
pub(super) fn known(id: &str) -> bool {
    TERMINALS.iter().any(|spec| spec.id == id)
}
pub(crate) fn discover(locations: &Locations, mac: bool, id: Option<&str>) -> Vec<InstalledEditor> {
    TERMINALS
        .iter()
        .filter(|spec| id.is_none_or(|id| id == spec.id) && (!spec.mac_only || mac))
        .filter_map(|spec| {
            let app = mac
                .then(|| {
                    locations
                        .applications
                        .iter()
                        .map(|base| base.join(spec.app))
                        .find(|path| {
                            !spec.app.is_empty() && path.join("Contents/Info.plist").is_file()
                        })
                })
                .flatten();
            let launcher = if let Some(app) = app {
                Launcher::MacApp(app)
            } else {
                Launcher::Command(
                    (!spec.binary.is_empty())
                        .then(|| locations.executable(spec.binary))
                        .flatten()?,
                )
            };
            Some(InstalledEditor {
                id: spec.id.into(),
                name: spec.name.into(),
                launcher,
            })
        })
        .collect()
}
pub(crate) fn command(tool: &InstalledEditor, root: &Path) -> Command {
    let mut command = match &tool.launcher {
        Launcher::MacApp(app) => {
            let mut command = Command::new("/usr/bin/open");
            if !["terminal", "iterm"].contains(&tool.id.as_str()) {
                command.arg("-n");
            }
            command.arg("-a").arg(app);
            if ["terminal", "iterm"].contains(&tool.id.as_str()) {
                command.arg("--").arg(root);
                return finish(command, root);
            }
            command.arg("--args");
            command
        }
        Launcher::Command(binary) => Command::new(binary),
        Launcher::Desktop { .. } => {
            unreachable!("Terminal desktop entries have no portable cwd contract")
        }
    };
    match tool.id.as_str() {
        "kitty" => {
            command.arg("--directory").arg(root);
        }
        "wezterm" => {
            command.args(["start", "--cwd"]).arg(root);
        }
        "konsole" => {
            command.arg("--workdir").arg(root);
        }
        "xterm" => (),
        // Ghostty's CLI configuration flags use key=value; the entire value remains one argv item.
        "ghostty" => {
            command.arg("--window-inherit-working-directory=false");
            if matches!(tool.launcher, Launcher::Command(_)) && !cfg!(target_os = "macos") {
                command.arg("--gtk-single-instance=false");
            }
            let mut value = std::ffi::OsString::from("--working-directory=");
            value.push(root);
            command.arg(value);
        }
        _ => {
            command.arg("--working-directory").arg(root);
        }
    }
    finish(command, root)
}
fn finish(mut command: Command, root: &Path) -> Command {
    command
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    super::clean_git_environment(&mut command);
    command
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};
    #[test]
    fn linux_discovery_never_assumes_a_macos_terminal_or_uses_non_executable_files() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("Terminal.app/Contents");
        fs::create_dir_all(&app).unwrap();
        fs::write(app.join("Info.plist"), "").unwrap();
        fs::write(dir.path().join("kitty"), "#!/bin/sh\n").unwrap();
        let places = Locations {
            binaries: vec![dir.path().into()],
            applications: vec![dir.path().into()],
            desktops: vec![],
        };
        assert!(discover(&places, false, None).is_empty());
        fs::set_permissions(dir.path().join("kitty"), fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            discover(&places, false, None)
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["kitty"]
        );
        assert_eq!(discover(&places, true, Some("terminal"))[0].id, "terminal");
    }
    #[test]
    fn every_strategy_passes_an_unusual_folder_as_data() {
        let path = Path::new("/tmp/a folder/'quoted'\n$(touch must-not-run); --flag");
        for spec in TERMINALS.iter().filter(|s| !s.mac_only) {
            let tool = InstalledEditor {
                id: spec.id.into(),
                name: spec.name.into(),
                launcher: Launcher::Command("/bin/true".into()),
            };
            let command = command(&tool, path);
            let command = command.as_std();
            let args = command.get_args().collect::<Vec<_>>();
            assert_eq!(command.get_current_dir(), Some(path));
            if spec.id == "ghostty" {
                assert_eq!(
                    args.last().unwrap(),
                    &std::ffi::OsStr::new(&format!("--working-directory={}", path.display()))
                );
            } else if spec.id != "xterm" {
                assert_eq!(args.last().unwrap(), &path.as_os_str());
            }
            assert!(!args.iter().any(|arg| arg == &"-c"));
        }
        let tool = InstalledEditor {
            id: "terminal".into(),
            name: "Terminal".into(),
            launcher: Launcher::MacApp("/System/Applications/Utilities/Terminal.app".into()),
        };
        let command = command(&tool, path);
        assert_eq!(command.as_std().get_args().last(), Some(path.as_os_str()));
    }
}
#[cfg(test)]
mod launch_tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};
    #[tokio::test]
    async fn real_launcher_gets_the_exact_folder_and_argv_without_shell_interpolation() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir
            .path()
            .join("folder 'quoted'\n$(touch never-created); --odd");
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let launcher = dir.path().join("gnome-terminal");
        fs::write(
            &launcher,
            "#!/bin/sh\nprintf '%s\\0' \"$PWD\" \"$@\" > received.bin\n",
        )
        .unwrap();
        fs::set_permissions(&launcher, fs::Permissions::from_mode(0o755)).unwrap();
        let places = Locations {
            binaries: vec![dir.path().into()],
            applications: vec![],
            desktops: vec![],
        };
        let tool = discover(&places, false, None).pop().unwrap();
        assert!(!root.join("received.bin").exists());
        let child = command(&tool, &root).spawn().unwrap();
        super::super::check_launch(&tool, child).await.unwrap();
        let data = fs::read(root.join("received.bin")).unwrap();
        let args = data.split(|b| *b == 0).collect::<Vec<_>>();
        assert_eq!(args[0], root.as_os_str().as_encoded_bytes());
        assert_eq!(args[1], b"--working-directory");
        assert_eq!(args[2], root.as_os_str().as_encoded_bytes());
        assert!(!root.join("never-created").exists());
        fs::write(&launcher, "#!/bin/sh\nexit 42\n").unwrap();
        assert!(
            super::super::check_launch(&tool, command(&tool, &root).spawn().unwrap())
                .await
                .unwrap_err()
                .contains("42")
        );
    }
}

/// Only strategies whose documented argv accepts a command vector after the cwd options.
pub(crate) fn agent_terminals() -> Vec<InstalledEditor> {
    discover(&Locations::system(), cfg!(target_os = "macos"), None)
        .into_iter()
        .filter(|t| {
            [
                "kitty",
                "wezterm",
                "alacritty",
                "ghostty",
                "gnome-terminal",
                "konsole",
                "xterm",
            ]
            .contains(&t.id.as_str())
        })
        .collect()
}
pub(crate) fn agent_command(
    tool: &InstalledEditor,
    root: &Path,
    argv: &[std::ffi::OsString],
) -> Command {
    let mut command = command(tool, root);
    if tool.id == "kitty" {
        command.args(["--session", "none"]);
    }
    if tool.id == "wezterm" {
        command.args([
            "--always-new-process",
            "--no-auto-connect",
            "--domain",
            "local",
        ]);
    }
    command.arg(
        if ["kitty", "wezterm", "gnome-terminal"].contains(&tool.id.as_str()) {
            "--"
        } else {
            "-e"
        },
    );
    command.args(argv);
    command
}

#[cfg(test)]
mod agent_tests {
    use super::*;
    #[test]
    fn agent_terminal_commands_keep_arguments_literal_and_disable_kitty_startup_sessions() {
        let root = Path::new("/repo/quotes ' 世界\n folder");
        let argv = vec![
            std::ffi::OsString::from("/tools/my agent"),
            std::ffi::OsString::from("Review $(touch injected); `whoami` 'quotes'\ntext"),
        ];
        for id in [
            "kitty",
            "wezterm",
            "alacritty",
            "ghostty",
            "gnome-terminal",
            "konsole",
            "xterm",
        ] {
            let tool = InstalledEditor {
                id: id.into(),
                name: id.into(),
                launcher: Launcher::Command("/fake-terminal".into()),
            };
            let command = agent_command(&tool, root, &argv);
            let args = command.as_std().get_args().collect::<Vec<_>>();
            assert_eq!(
                &args[args.len() - 2..],
                &[argv[0].as_os_str(), argv[1].as_os_str()]
            );
            if id == "kitty" {
                assert!(args.windows(2).any(|a| a == ["--session", "none"]));
            }
        }
    }
}

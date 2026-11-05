use super::*;
use discovery::Locations;
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

fn executable(path: &Path, script: &str) {
    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}
fn locations(root: &Path) -> Locations {
    Locations {
        binaries: vec![root.into()],
        applications: vec![root.into()],
        desktops: vec![root.into()],
    }
}
#[test]
fn finds_apps_without_cli_and_deduplicates_command_installations() {
    let dir = tempfile::tempdir().unwrap();
    let app = dir.path().join("Cursor.app/Contents");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("Info.plist"), "plist").unwrap();
    executable(&dir.path().join("cursor"), "#!/bin/sh\nexit 0\n");
    let editors = discover(&locations(dir.path()));
    assert_eq!(editors.len(), 1);
    assert_eq!(editors[0].id, "cursor");
    assert!(matches!(editors[0].launcher, Launcher::MacApp(_)));
}
#[test]
fn ignores_non_executable_and_broken_commands() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("code"), "not executable").unwrap();
    std::os::unix::fs::symlink(dir.path().join("missing"), dir.path().join("cursor")).unwrap();
    assert!(discover(&locations(dir.path())).is_empty());
}
#[test]
fn discovers_linux_desktop_entries_without_a_cli_and_ignores_hidden_entries() {
    let dir = tempfile::tempdir().unwrap();
    executable(&dir.path().join("gio"), "#!/bin/sh\nexit 0\n");
    fs::write(
        dir.path().join("com.jetbrains.RustRover.desktop"),
        "[Desktop Entry]\nType=Application\nExec=flatpak run com.jetbrains.RustRover\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("code.desktop"),
        "[Desktop Entry]\nType=Application\nHidden=true\n",
    )
    .unwrap();
    let editors = discover(&locations(dir.path()));
    assert_eq!(editors.len(), 1);
    assert_eq!(editors[0].id, "rustrover");
    assert!(matches!(editors[0].launcher, Launcher::Desktop { .. }));
}
#[test]
fn launcher_arguments_preserve_repository_paths() {
    let root = Path::new("/tmp/repo with spaces; $(touch bad)");
    for (launcher, expected) in [
        (
            Launcher::Command("/bin/code".into()),
            vec![root.as_os_str()],
        ),
        (
            Launcher::MacApp("/Applications/Cursor.app".into()),
            vec![
                std::ffi::OsStr::new("-a"),
                std::ffi::OsStr::new("/Applications/Cursor.app"),
                root.as_os_str(),
            ],
        ),
        (
            Launcher::Desktop {
                gio: "/bin/gio".into(),
                entry: "/apps/code.desktop".into(),
            },
            vec![
                std::ffi::OsStr::new("launch"),
                std::ffi::OsStr::new("/apps/code.desktop"),
                root.as_os_str(),
            ],
        ),
    ] {
        let cmd = command(&launcher, root);
        assert_eq!(cmd.as_std().get_args().collect::<Vec<_>>(), expected);
    }
}
#[tokio::test]
async fn launches_the_actual_folder_as_one_argument_and_reports_failures() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project with 'quotes' and $(commands)");
    fs::create_dir(&root).unwrap();
    let binary = dir.path().join("code");
    executable(&binary, "#!/bin/sh\nprintf '%s\\n' \"$@\" > launched.txt\n");
    let editor = InstalledEditor {
        id: "vscode".into(),
        name: "VS Code".into(),
        launcher: Launcher::Command(binary.clone()),
    };
    launch(&editor, &root).await.unwrap();
    assert_eq!(
        fs::read_to_string(root.join("launched.txt")).unwrap(),
        format!("{}\n", root.display())
    );
    executable(&binary, "#!/bin/sh\nexit 42\n");
    assert!(launch(&editor, &root).await.unwrap_err().contains("42"));
    fs::remove_file(binary).unwrap();
    assert!(launch(&editor, &PathBuf::from("/missing")).await.is_err());
}

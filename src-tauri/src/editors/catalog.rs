pub struct Editor {
    pub id: &'static str,
    pub name: &'static str,
    pub commands: &'static [&'static str],
    pub apps: &'static [&'static str],
    pub desktops: &'static [&'static str],
}

pub const EDITORS: &[Editor] = &[
    Editor {
        id: "vscode",
        name: "Visual Studio Code",
        commands: &["code"],
        apps: &["Visual Studio Code.app"],
        desktops: &[
            "code.desktop",
            "code_code.desktop",
            "com.visualstudio.code.desktop",
        ],
    },
    Editor {
        id: "vscode-insiders",
        name: "VS Code Insiders",
        commands: &["code-insiders"],
        apps: &["Visual Studio Code - Insiders.app"],
        desktops: &["code-insiders.desktop"],
    },
    Editor {
        id: "cursor",
        name: "Cursor",
        commands: &["cursor"],
        apps: &["Cursor.app"],
        desktops: &["cursor.desktop", "Cursor.desktop"],
    },
    Editor {
        id: "windsurf",
        name: "Windsurf",
        commands: &["windsurf"],
        apps: &["Windsurf.app"],
        desktops: &["windsurf.desktop"],
    },
    Editor {
        id: "zed",
        name: "Zed",
        commands: &["zed", "zeditor"],
        apps: &["Zed.app", "Zed Preview.app"],
        desktops: &["dev.zed.Zed.desktop", "dev.zed.Zed-Preview.desktop"],
    },
    Editor {
        id: "sublime",
        name: "Sublime Text",
        commands: &["subl", "sublime_text"],
        apps: &["Sublime Text.app"],
        desktops: &[
            "sublime_text.desktop",
            "sublime-text_subl.desktop",
            "com.sublimetext.three.desktop",
        ],
    },
    Editor {
        id: "idea",
        name: "IntelliJ IDEA",
        commands: &[
            "idea",
            "intellij-idea",
            "intellij-idea-community",
            "intellij-idea-ultimate",
        ],
        apps: &[
            "IntelliJ IDEA.app",
            "IntelliJ IDEA CE.app",
            "IntelliJ IDEA Ultimate.app",
        ],
        desktops: &[
            "jetbrains-idea.desktop",
            "jetbrains-idea-ce.desktop",
            "com.jetbrains.IntelliJ-IDEA-Community.desktop",
            "com.jetbrains.IntelliJ-IDEA-Ultimate.desktop",
        ],
    },
    Editor {
        id: "pycharm",
        name: "PyCharm",
        commands: &["pycharm", "pycharm-community", "pycharm-professional"],
        apps: &["PyCharm.app", "PyCharm CE.app"],
        desktops: &[
            "jetbrains-pycharm.desktop",
            "jetbrains-pycharm-ce.desktop",
            "com.jetbrains.PyCharm-Community.desktop",
            "com.jetbrains.PyCharm-Professional.desktop",
        ],
    },
    Editor {
        id: "webstorm",
        name: "WebStorm",
        commands: &["webstorm"],
        apps: &["WebStorm.app"],
        desktops: &[
            "jetbrains-webstorm.desktop",
            "com.jetbrains.WebStorm.desktop",
        ],
    },
    Editor {
        id: "clion",
        name: "CLion",
        commands: &["clion"],
        apps: &["CLion.app"],
        desktops: &["jetbrains-clion.desktop", "com.jetbrains.CLion.desktop"],
    },
    Editor {
        id: "goland",
        name: "GoLand",
        commands: &["goland"],
        apps: &["GoLand.app"],
        desktops: &["jetbrains-goland.desktop", "com.jetbrains.GoLand.desktop"],
    },
    Editor {
        id: "rustrover",
        name: "RustRover",
        commands: &["rustrover"],
        apps: &["RustRover.app"],
        desktops: &[
            "jetbrains-rustrover.desktop",
            "com.jetbrains.RustRover.desktop",
        ],
    },
    Editor {
        id: "rider",
        name: "Rider",
        commands: &["rider"],
        apps: &["Rider.app"],
        desktops: &["jetbrains-rider.desktop", "com.jetbrains.Rider.desktop"],
    },
    Editor {
        id: "phpstorm",
        name: "PhpStorm",
        commands: &["phpstorm"],
        apps: &["PhpStorm.app"],
        desktops: &[
            "jetbrains-phpstorm.desktop",
            "com.jetbrains.PhpStorm.desktop",
        ],
    },
    Editor {
        id: "rubymine",
        name: "RubyMine",
        commands: &["rubymine"],
        apps: &["RubyMine.app"],
        desktops: &[
            "jetbrains-rubymine.desktop",
            "com.jetbrains.RubyMine.desktop",
        ],
    },
];

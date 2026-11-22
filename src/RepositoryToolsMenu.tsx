import { Menu } from "@base-ui/react/menu";
import { Bot, GitFork, Terminal, Wrench } from "lucide-react";
import { useCommands } from "../vendor/diffle/src/client/commands/CommandProvider";
import { uiText } from "../vendor/diffle/src/client/i18n/runtime";
export default function RepositoryToolsMenu() {
  const registry = useCommands();
  const items = [
    { id: "app.agents", label: uiText("Agents and review"), Icon: Bot },
    {
      id: "app.embeddedTerminal",
      label: uiText("Embedded terminal"),
      Icon: Terminal,
    },
    {
      id: "app.worktrees",
      label: uiText("Worktrees and external tools"),
      Icon: GitFork,
    },
  ];
  return (
    <Menu.Root defaultOpen>
      <Menu.Trigger
        id="repository-tools-toggle"
        aria-label={uiText("Repository tools")}
        title={uiText("Repository tools")}
      >
        <Wrench size={15} />
        <span className="action-label">{uiText("Tools")}</span>
      </Menu.Trigger>
      <Menu.Portal>
        <Menu.Positioner
          side="bottom"
          align="end"
          sideOffset={6}
          className="repository-tools-positioner"
        >
          <Menu.Popup
            className="repository-tools-menu"
            aria-label={uiText("Repository tools")}
          >
            {items.map(({ id, label, Icon }) => (
              <Menu.Item key={id} onClick={() => registry?.execute(id)}>
                <Icon size={15} />
                {label}
              </Menu.Item>
            ))}
          </Menu.Popup>
        </Menu.Positioner>
      </Menu.Portal>
    </Menu.Root>
  );
}

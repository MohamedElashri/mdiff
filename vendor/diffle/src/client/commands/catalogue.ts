import { message, type Message } from '@/client/i18n/runtime.js';

export type CommandScope = 'app' | 'review';
export interface CommandDefinition {
  id: string;
  label: Message;
  scope: CommandScope;
  shortcut?: string;
  /** Fixed Vim aliases continue to use the existing chord/count engine. */
  key?: string;
  allowInInput?: boolean;
}

const command = (
  id: string,
  label: string,
  key?: string,
  shortcut?: string,
  allowInInput = false,
): CommandDefinition => ({
  id,
  label: message(`command.${id}`, label),
  scope: id.startsWith('app.') ? 'app' : 'review',
  key,
  shortcut,
  allowInInput,
});

/** Lightweight metadata can be shown in preferences without importing the review store or renderer. */
export const COMMANDS: readonly CommandDefinition[] = [
  command('app.newTab', 'New review tab', undefined, 'Mod+T'),
  command('app.nextTab', 'Next review tab', undefined, 'Ctrl+PageDown'),
  command('app.previousTab', 'Previous review tab', undefined, 'Ctrl+PageUp'),
  command('app.closeTab', 'Close review tab', undefined, 'Mod+Alt+W'),
  command('app.reopenTab', 'Reopen closed review tab', undefined, 'Mod+Shift+T'),
  command('app.gitOperations', 'Git operations and recovery'),
  command('app.worktrees', 'Browse linked worktrees'),
  command('app.agents', 'Agents and review'),
  command('app.attention', 'Needs attention inbox'),
  command('app.embeddedTerminal', 'Show embedded terminal'),
  command('app.terminal', 'Open folder in terminal'),
  command('app.editor', 'Open folder in editor'),
  command('app.hosting', 'Hosting reviews and checks'),
  command('app.history', 'Browse repository history'),
  command('app.changes', 'Return to changes'),
  command('app.palette', 'Open command palette', undefined, 'Mod+K'),
  command('app.shortcuts', 'Customize keyboard shortcuts'),
  command('app.openRepository', 'Open repository', undefined, 'Mod+O'),
  command('app.preferences', 'Open preferences', undefined, 'Mod+,'),
  command('app.home', 'Recent reviews'),
  command('review.settings', 'Review settings', undefined, 'Mod+Shift+,'),
  command('review.files', 'Find file', 'gf', 'Mod+P', true),
  command('review.tree', 'Toggle file tree', undefined, 'Mod+B'),
  command('review.panel', 'Toggle review inspector', undefined, 'Mod+Shift+B'),
  command('review.focusTree', 'Focus file tree', undefined, 'Mod+Shift+E'),
  command('review.back', 'Go back', undefined, 'Ctrl+O'),
  command('review.forward', 'Go forward', undefined, 'Ctrl+I'),
  command('review.find', 'Search current file', '/', 'Mod+F'),
  command('review.findChanged', 'Search changed files', 'g/'),
  command('review.nextFile', 'Next file', 'J'),
  command('review.previousFile', 'Previous file', 'K'),
  command('review.nextHunk', 'Next change', ']'),
  command('review.previousHunk', 'Previous change', '['),
  command('review.viewed', 'Toggle file viewed', 'v'),
  command('review.viewedAbove', 'Toggle viewed above', 'gv'),
  command('review.expand', 'Expand current file', 'zo'),
  command('review.collapse', 'Collapse current file', 'zc'),
  command('review.expandAll', 'Expand all files', 'zO'),
  command('review.collapseAll', 'Collapse all files', 'zC'),
  command('review.layout', 'Toggle split or unified diff', 's'),
  command('review.split', 'Use split diff'),
  command('review.unified', 'Use unified diff'),
  command('review.wrap', 'Toggle line wrapping', 'zw'),
  command('review.theme', 'Cycle color mode', 't'),
  command('review.graph', 'Toggle change graph', 'M'),
  command('review.showGraph', 'Show change graph'),
  command('review.showDiff', 'Show diff'),
  command('review.commentFile', 'Comment on current file', 'C'),
  command('review.comment', 'Comment on selection', 'c'),
  command('review.resolve', 'Resolve or reopen thread', 'R'),
  command('review.copyComments', 'Copy comments as agent prompt', 'yy'),
  command('review.copyPath', 'Copy file path', 'yp'),
  command('review.permalink', 'Copy GitHub permalink', 'yl'),
  command('review.compare', 'Choose comparison', 'm'),
  command('review.github', 'Open GitHub review panel', 'o'),
  command('review.nextCommit', 'Next commit', '>'),
  command('review.previousCommit', 'Previous commit', '<'),
  command('review.reload', 'Reload moved comparison', 'r'),
  command('review.symbols', 'Find file symbols', 'gs'),
  command('review.workspaceSymbols', 'Find repository symbols', 'gS'),
  command('review.definition', 'Go to definition', 'gd'),
  command('review.typeDefinition', 'Go to type definition', 'gy'),
  command('review.references', 'Find references', 'gA'),
  command('review.hover', 'Show symbol information', 'gh'),
  command('review.fullFile', 'View full file', 'F'),
  command('review.iteration', 'Compare latest iterations', 'ii'),
  command('review.help', 'Show review shortcuts', '?'),
];

export const COMMAND_BY_ID = new Map(COMMANDS.map((entry) => [entry.id, entry]));
export const COMMAND_BY_KEY = new Map(COMMANDS.filter((entry) => entry.key).map((entry) => [entry.key!, entry]));

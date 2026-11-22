import { workspaceMessages } from '../../../src/workspaces/messages.js';
import { uiMessages } from './ui-messages.js';
/** Run with `TSX_TSCONFIG_PATH=tsconfig.client.json node --import tsx scripts/extract-messages.ts > /tmp/mdiff-en.json` from vendor/diffle. */
import { COMMANDS } from '../src/client/commands/catalogue.js';
import { historyMessages } from '../src/client/history/messages.js';
import { messages } from '../src/client/i18n/messages.js';

// The command helper and message() definitions form the initial extraction catalogue.
// Add future feature catalogues here; this does not import the store, DOM or transport.
const catalogue = [
  ...Object.values(workspaceMessages),
  ...Object.values(historyMessages),
  ...Object.values(messages),
  ...COMMANDS.map((command) => command.label),
];
const output: Record<string, (typeof catalogue)[number]['text']> = {};
for (const entry of catalogue) {
  if (Object.hasOwn(output, entry.id)) throw new Error(`Duplicate message ID: ${entry.id}`);
  output[entry.id] = entry.text;
}
Object.assign(output, uiMessages());
process.stdout.write(
  JSON.stringify(Object.fromEntries(Object.entries(output).sort(([a], [b]) => a.localeCompare(b, 'en'))), null, 2) +
    '\n',
);

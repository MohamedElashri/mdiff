import { useViewActions } from '@/client/views/state.js';
import { useCommandAction } from '@/client/commands/CommandProvider.js';
import { useStore } from '@/client/store.js';
import { SegmentedControl, ToggleButton } from '@/client/ui/ToggleButton.js';
import { listViews, useViews, type ViewDef } from './state.js';

/** Which view takes the main pane; one that cannot be shown now names why. */
export function ViewSwitch() {
  return (
    <SegmentedControl>
      {listViews().map((view) => (
        <ViewButton key={view.id} view={view} />
      ))}
    </SegmentedControl>
  );
}

function ViewButton({ view }: { view: ViewDef }) {
  const { showView } = useViewActions();

  const selected = useViews((s) => s.active === view.id);
  const blocked = useStore((s) => view.blocked?.(s) ?? null);
  const command = useCommandAction(view.id === 'graph' ? 'review.showGraph' : 'review.showDiff', () =>
    showView(view.id),
  );
  const Icon = view.icon;
  return (
    <ToggleButton
      selected={selected}
      disabled={!selected && blocked != null}
      onClick={command.run}
      aria-label={view.label}
      title={blocked ?? view.title}
    >
      <Icon size="0.875rem" /> <span className="max-xl:hidden">{view.label}</span>
    </ToggleButton>
  );
}

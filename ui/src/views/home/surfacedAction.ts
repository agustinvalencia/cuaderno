import type { ActionListEntry } from "../../api/bindings/ActionListEntry";
import type { EnergyLevel } from "../../api/bindings/EnergyLevel";
import type { OrientationProject } from "../../api/bindings/OrientationProject";

// The energy filter's no-match rule (design law): a card never
// blanks. With a filter active, surface the first matching bullet;
// otherwise keep the best-available action with a muted note —
// low-energy moments must not be greeted by empty cards.
export function surfacedAction(
  project: OrientationProject,
  energy: EnergyLevel | null,
): { action: ActionListEntry | null; matchedFilter: boolean } {
  if (energy) {
    const match = project.actions.find((a) => a.energy === energy);
    if (match) return { action: match, matchedFilter: true };
  }
  const best =
    project.actions[0] ??
    (project.top_action
      ? { text: project.top_action.text, energy: project.top_action.energy, attached: null }
      : null);
  return { action: best, matchedFilter: energy === null };
}

// Stand-in for routes whose views land in later milestones. Named so
// nobody mistakes a blank screen for a bug.
export default function Placeholder({ view, milestone }: { view: string; milestone: string }) {
  return (
    <div className="p-8">
      <h1 className="text-lg font-medium text-ink">{view}</h1>
      <p className="mt-2 text-sm text-ink-muted">This view arrives in {milestone}.</p>
    </div>
  );
}

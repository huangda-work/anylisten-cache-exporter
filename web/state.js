export function reconcileSelection(selected, previous, tracks) {
  const current = new Set(tracks.map((track) => track.id));
  const next = new Set([...selected].filter((id) => current.has(id)));
  for (const track of tracks) {
    if (!track.needsNetwork && !previous.has(track.id)) next.add(track.id);
  }
  return next;
}

export function toggleVisibleSelection(selected, visible) {
  const next = new Set(selected);
  const allSelected = visible.length > 0 && visible.every((track) => next.has(track.id));
  for (const track of visible) {
    if (allSelected) next.delete(track.id);
    else next.add(track.id);
  }
  return next;
}

export function filterTracks(tracks, source, query) {
  const term = query.trim().toLocaleLowerCase();
  return tracks.filter((track) => {
    if (source === "cached" && track.needsNetwork) return false;
    if (source === "remote" && !track.needsNetwork) return false;
    return !term || `${track.title} ${track.artist}`.toLocaleLowerCase().includes(term);
  });
}

export function selectionSummary(selected, statuses) {
  const summary = { total: selected.size, toSave: 0, saved: 0, uncertain: 0 };
  for (const id of selected) {
    if (statuses[id] === "saved") summary.saved++;
    else if (statuses[id] === "notSaved") summary.toSave++;
    else summary.uncertain++;
  }
  return summary;
}

export function selectOnlyUnsaved(tracks, statuses) {
  return new Set(tracks.filter((track) => statuses[track.id] === "notSaved").map((track) => track.id));
}

export function retryFailedIds(failures) {
  return [...new Set(failures.map((failure) => failure.id))];
}

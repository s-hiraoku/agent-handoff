const searchInput = document.querySelector("[data-command-search]");
const filterButtons = Array.from(document.querySelectorAll("[data-command-filter]"));
const rows = Array.from(document.querySelectorAll("[data-command-row]"));
const resultCount = document.querySelector("[data-result-count]");
const inspectorCommand = document.querySelector("[data-inspector-command]");
const inspectorUse = document.querySelector("[data-inspector-use]");
const inspectorRecovery = document.querySelector("[data-inspector-recovery]");

let activeFilter = "all";

function updateInspector(row) {
  if (!row || !inspectorCommand || !inspectorUse || !inspectorRecovery) {
    return;
  }

  inspectorCommand.textContent = row.dataset.command || "";
  inspectorUse.textContent = row.dataset.use || "";
  inspectorRecovery.textContent = row.dataset.recovery || "";
}

function applyFilters() {
  const query = (searchInput?.value || "").trim().toLowerCase();
  let visible = 0;
  let firstVisible = null;

  for (const row of rows) {
    const categoryMatch = activeFilter === "all" || row.dataset.category === activeFilter;
    const text = row.textContent.toLowerCase();
    const queryMatch = !query || text.includes(query);
    const shouldShow = categoryMatch && queryMatch;
    row.hidden = !shouldShow;
    if (shouldShow) {
      visible += 1;
      firstVisible ||= row;
    }
  }

  if (resultCount) {
    resultCount.textContent = `${visible} command${visible === 1 ? "" : "s"}`;
  }

  updateInspector(firstVisible);
}

for (const button of filterButtons) {
  button.addEventListener("click", () => {
    activeFilter = button.dataset.commandFilter || "all";
    for (const item of filterButtons) {
      item.setAttribute("aria-pressed", String(item === button));
    }
    applyFilters();
  });
}

for (const row of rows) {
  row.addEventListener("click", () => updateInspector(row));
  row.addEventListener("focusin", () => updateInspector(row));
}

searchInput?.addEventListener("input", applyFilters);
applyFilters();

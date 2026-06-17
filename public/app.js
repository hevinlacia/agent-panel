(function () {
  "use strict";

  const reportPath = window.__REPORT_PATH__;
  if (!reportPath) return;

  const checkboxes = document.querySelectorAll('input[type="checkbox"][data-cid]');
  const selectionInfo = document.getElementById("selection-info");
  const btnConfirm = document.getElementById("btn-confirm");
  const btnReject = document.getElementById("btn-reject");
  const btnSelectAll = document.getElementById("btn-select-all");
  const btnDeselectAll = document.getElementById("btn-deselect-all");

  function getSelectedIds() {
    return Array.from(checkboxes)
      .filter((cb) => cb.checked)
      .map((cb) => cb.dataset.cid);
  }

  function updateUI() {
    const ids = getSelectedIds();
    selectionInfo.textContent = `${ids.length} selected`;
    btnConfirm.disabled = ids.length === 0;
    btnReject.disabled = ids.length === 0;

    // Update card visual state
    document.querySelectorAll(".candidate-card").forEach((card) => {
      const cid = card.dataset.cid;
      const cb = card.querySelector(`input[data-cid="${cid}"]`);
      card.classList.toggle("checked", cb && cb.checked);
    });
  }

  async function submitSelection(mode) {
    const ids = getSelectedIds();
    if (ids.length === 0) return;

    btnConfirm.disabled = true;
    btnReject.disabled = true;

    try {
      const res = await fetch("/api/confirm", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          reportPath: reportPath,
          confirmedIds: mode === "confirm" ? ids : [],
          rejectedIds: mode === "reject" ? ids : [],
          mode: mode,
        }),
      });
      const data = await res.json();
      if (data.ok) {
        showToast(
          mode === "confirm"
            ? `✓ Confirmed ${ids.length} candidate(s): ${ids.join(", ")}`
            : `✗ Rejected ${ids.length} candidate(s): ${ids.join(", ")}`,
          "success"
        );
      } else {
        showToast("Error: " + (data.error || "unknown"), "error");
      }
    } catch (err) {
      showToast("Network error: " + err.message, "error");
    } finally {
      updateUI();
    }
  }

  function showToast(msg, type) {
    const toast = document.createElement("div");
    toast.className = "toast" + (type === "error" ? " error" : "");
    toast.textContent = msg;
    document.body.appendChild(toast);
    setTimeout(() => toast.remove(), 5000);
  }

  // Event listeners
  checkboxes.forEach((cb) => cb.addEventListener("change", updateUI));
  btnConfirm.addEventListener("click", () => submitSelection("confirm"));
  btnReject.addEventListener("click", () => submitSelection("reject"));
  btnSelectAll.addEventListener("click", () => {
    checkboxes.forEach((cb) => (cb.checked = true));
    updateUI();
  });
  btnDeselectAll.addEventListener("click", () => {
    checkboxes.forEach((cb) => (cb.checked = false));
    updateUI();
  });

  updateUI();
})();

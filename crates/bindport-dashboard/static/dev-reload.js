let version = "";

async function poll() {
  try {
    const response = await fetch("/assets/dev-version", { cache: "no-store" });
    if (response.ok) {
      const next = await response.text();
      if (version && next !== version) {
        window.location.reload();
        return;
      }
      version = next;
    }
  } finally {
    window.setTimeout(poll, 1000);
  }
}

poll();

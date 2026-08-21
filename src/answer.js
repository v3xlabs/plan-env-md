// In-document answer widget. Served from /_planenv/answer.js and injected by
// the viewer only for the document's owner.
//
// The document runs in a sandboxed opaque origin, so the session cookie is
// never sent. Writes carry the scoped key from data-key in an Authorization
// header, which keeps the request uncredentialed.

const script = document.currentScript ?? document.querySelector("script[data-planenv-key]");
const KEY = script.dataset.planenvKey;
const SLUG = script.dataset.planenvSlug;
// the API answers on the app origin, which is not the origin serving this
// document, so the address is handed over rather than resolved relative to it
const API = script.dataset.planenvApi;
const OTHER = "other";
const SAVE_DELAY = 600;

const source = document.getElementById("planenv-questions");
const questions = source ? JSON.parse(source.textContent) : [];

const state = new Map(
  questions.map((entry) => [
    entry.key,
    {
      selected: entry.answer ? entry.answer.selected : [],
      otherText: entry.answer ? (entry.answer.other_text ?? "") : "",
      notes: entry.answer ? (entry.answer.notes ?? "") : "",
      answeredAt: entry.answer ? entry.answer.answered_at : null,
    },
  ]),
);

const timers = new Map();

const answeredCount = () =>
  [...state.values()].filter((answer) => answer.selected.length > 0).length;

const updateCounter = () => {
  const counter = document.getElementById("planenv-answered");
  if (counter) counter.textContent = `${answeredCount()} of ${questions.length} answered`;
};

const request = (method, key, body) =>
  fetch(`${API}/api/docs/${encodeURIComponent(SLUG)}/answers/${encodeURIComponent(key)}`, {
    method,
    headers: {
      Authorization: `Bearer ${KEY}`,
      ...(body ? { "Content-Type": "application/json" } : {}),
    },
    body: body ? JSON.stringify(body) : undefined,
  });

const save = (card, key) => {
  clearTimeout(timers.get(key));
  timers.set(
    key,
    setTimeout(async () => {
      const answer = state.get(key);
      const status = card.querySelector(".planenv-status");
      const wroteOther = answer.selected.includes(OTHER);

      // an empty selection is a withdrawal, which is a different state from an
      // answer with nothing chosen
      if (answer.selected.length === 0) {
        status.textContent = "clearing";
        const response = await request("DELETE", key);
        status.textContent = response.ok ? "not answered" : "not saved";
        updateCounter();
        return;
      }
      if (wroteOther && !answer.otherText.trim()) {
        status.textContent = "write your answer";
        return;
      }

      status.textContent = "saving";
      let response;
      try {
        response = await request("PUT", key, {
          selected: answer.selected,
          other_text: wroteOther ? answer.otherText : null,
          notes: answer.notes || null,
        });
      } catch {
        status.textContent = "not saved, connection failed";
        return;
      }

      if (response.status === 401) {
        status.textContent = "reload to answer";
        return;
      }
      if (!response.ok) {
        status.textContent = `not saved (${response.status})`;
        return;
      }
      status.textContent = "saved just now";
      updateCounter();
    }, SAVE_DELAY),
  );
};

const toggle = (key, value, multiple) => {
  const answer = state.get(key);
  if (!multiple) {
    answer.selected = answer.selected[0] === value ? [] : [value];
    return;
  }
  answer.selected = answer.selected.includes(value)
    ? answer.selected.filter((entry) => entry !== value)
    : [...answer.selected, value];
};

const buildCard = (entry) => {
  const answer = state.get(entry.key);
  const card = document.createElement("aside");
  card.className = "planenv-q";
  card.dataset.key = entry.key;

  const head = document.createElement("header");
  head.innerHTML = `<b class="planenv-q-key"></b><span class="planenv-q-prompt"></span>`;
  head.querySelector(".planenv-q-key").textContent = entry.key;
  head.querySelector(".planenv-q-prompt").textContent = entry.prompt;
  card.append(head);

  if (entry.detail) {
    const detail = document.createElement("p");
    detail.className = "planenv-q-detail";
    detail.textContent = entry.detail;
    card.append(detail);
  }

  const list = document.createElement("div");
  list.className = "planenv-opts";
  card.append(list);

  const notes = document.createElement("textarea");
  notes.className = "planenv-notes";
  notes.rows = 2;
  notes.placeholder = "note";
  notes.value = answer.notes;
  notes.hidden = !answer.notes;

  const render = () => {
    list.textContent = "";
    for (const option of entry.options) {
      const row = document.createElement("div");
      row.className = "planenv-opt";
      if (answer.selected.includes(option.value)) row.classList.add("is-on");

      const pick = document.createElement("button");
      pick.type = "button";
      pick.className = "planenv-pick";
      pick.innerHTML = `<i class="planenv-mark${entry.multiple ? " is-box" : ""}"></i>`;
      const label = document.createElement("span");
      label.className = "planenv-label";
      label.textContent = option.label;
      pick.append(label);
      if (option.detail) {
        const hint = document.createElement("small");
        hint.textContent = option.detail;
        pick.append(hint);
      }
      pick.addEventListener("click", () => {
        toggle(entry.key, option.value, entry.multiple);
        render();
        save(card, entry.key);
      });

      const note = document.createElement("button");
      note.type = "button";
      note.className = "planenv-note-btn";
      note.textContent = "add note";
      // adding a note is also a way of choosing: it selects the row it sits on
      note.addEventListener("click", () => {
        if (!answer.selected.includes(option.value)) {
          toggle(entry.key, option.value, entry.multiple);
        }
        notes.hidden = false;
        render();
        notes.focus();
        save(card, entry.key);
      });

      row.append(pick, note);
      list.append(row);
    }

    // the ghost row is always offered; the product owns it, not the document
    const ghost = document.createElement("div");
    ghost.className = "planenv-opt planenv-ghost";
    const wroteOther = answer.selected.includes(OTHER);
    if (wroteOther) ghost.classList.add("is-on");

    const field = document.createElement("textarea");
    field.className = "planenv-other";
    field.rows = 2;
    field.placeholder = "write your own answer";
    field.value = answer.otherText;
    field.addEventListener("input", () => {
      answer.otherText = field.value;
      if (!answer.selected.includes(OTHER)) toggle(entry.key, OTHER, entry.multiple);
      save(card, entry.key);
    });

    if (wroteOther || answer.otherText) {
      const mark = document.createElement("i");
      mark.className = `planenv-mark${entry.multiple ? " is-box" : ""}`;
      ghost.append(mark, field);
    } else {
      const open = document.createElement("button");
      open.type = "button";
      open.className = "planenv-pick planenv-open";
      open.innerHTML = `<i class="planenv-mark is-ghost"></i>`;
      const label = document.createElement("span");
      label.className = "planenv-label";
      label.textContent = "write your own answer";
      open.append(label);
      open.addEventListener("click", () => {
        toggle(entry.key, OTHER, entry.multiple);
        render();
        card.querySelector(".planenv-other")?.focus();
      });
      ghost.append(open);
    }
    list.append(ghost);
  };

  render();
  notes.addEventListener("input", () => {
    answer.notes = notes.value;
    save(card, entry.key);
  });
  card.append(notes);

  const foot = document.createElement("footer");
  const status = document.createElement("span");
  status.className = "planenv-status";
  status.textContent = answer.answeredAt ? "answered" : "not answered";
  const clear = document.createElement("button");
  clear.type = "button";
  clear.className = "planenv-clear";
  clear.textContent = "clear";
  clear.addEventListener("click", () => {
    answer.selected = [];
    answer.otherText = "";
    answer.notes = "";
    notes.value = "";
    notes.hidden = true;
    render();
    save(card, entry.key);
  });
  foot.append(status, clear);
  card.append(foot);
  return card;
};

let panel = null;
for (const entry of questions) {
  const card = buildCard(entry);
  const anchor = entry.anchor ? document.getElementById(entry.anchor) : null;
  if (anchor) {
    // insert a sibling; never rewrite, reorder or restyle what the agent wrote
    anchor.after(card);
    continue;
  }
  if (!panel) {
    panel = document.createElement("section");
    panel.id = "planenv-panel";
    panel.innerHTML = `<h2>Questions</h2>`;
    document.body.append(panel);
  }
  panel.append(card);
}

updateCounter();

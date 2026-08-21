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
const cards = new Map();
const markers = new Map();
let current = questions[0]?.key;

const isAnswered = (key) => state.get(key).selected.length > 0;
const answeredCount = () => questions.filter((entry) => isAnswered(entry.key)).length;

const paint = (key) => {
  const answered = isAnswered(key);
  cards.get(key)?.classList.toggle("is-answered", answered);
  markers.get(key)?.classList.toggle("is-answered", answered);
};

const updateProgress = () => {
  const progress = document.getElementById("planenv-progress");
  if (!progress) return;

  const count = answeredCount();
  const total = questions.length;
  const label = document.getElementById("planenv-answered");
  if (label) label.textContent = `${count} of ${total}`;
  progress.title = `${count} of ${total} answered`;
  progress.classList.toggle("is-done", count === total);

  const track = document.getElementById("planenv-track");
  if (!track) return;
  [...track.children].forEach((segment, index) => {
    segment.classList.toggle("is-on", isAnswered(questions[index].key));
  });
};

const reveal = (key) => {
  const card = cards.get(key);
  if (!card) return;
  current = key;
  card.scrollIntoView({ behavior: "smooth", block: "center" });
  card.focus({ preventScroll: true });
};

/// The arrows walk what is still open, since that is what the count beside them
/// counts. With nothing left they walk everything, so the control stays a way
/// back to what was decided.
const step = (direction) => {
  const open = questions.filter((entry) => !isAnswered(entry.key));
  const walk = open.length > 0 ? open : questions;
  const here = walk.findIndex((entry) => entry.key === current);
  reveal(walk[(here + direction + walk.length) % walk.length].key);
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

      // an empty selection is a withdrawal, which is a different state from an
      // answer with nothing chosen
      if (answer.selected.length === 0) {
        status.textContent = "clearing";
        const response = await request("DELETE", key);
        status.textContent = response.ok ? "not answered" : "not saved";
        updateProgress();
        return;
      }

      status.textContent = "saving";
      let response;
      try {
        response = await request("PUT", key, {
          selected: answer.selected,
          other_text: answer.selected.includes(OTHER) ? answer.otherText : null,
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
      updateProgress();
    }, SAVE_DELAY),
  );
};

const buildCard = (entry, ordinal) => {
  const answer = state.get(entry.key);
  const card = document.createElement("aside");
  card.className = "planenv-q";
  card.dataset.key = entry.key;
  card.tabIndex = -1;

  const head = document.createElement("header");
  head.innerHTML =
    `<b class="planenv-q-key"></b><span class="planenv-q-prompt"></span><span class="planenv-q-how"></span>`;
  head.querySelector(".planenv-q-key").textContent = ordinal;
  head.querySelector(".planenv-q-prompt").textContent = entry.prompt;
  head.querySelector(".planenv-q-how").textContent = entry.multiple ? "choose any" : "choose one";
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

  const reply = document.createElement("textarea");
  reply.className = "planenv-reply";
  reply.rows = 2;
  reply.placeholder = "write your own answer, or add a note";
  reply.value = answer.otherText || answer.notes;

  // The declared options the reader has picked. The written answer is not one
  // of them: it is derived below, because what the text means depends on
  // whether anything else is chosen.
  const picked = new Set(answer.selected.filter((value) => value !== OTHER));

  // One field does the job the old widget split across a hidden note button and
  // a separate written answer row. With nothing picked the text is the answer
  // itself, and the server takes it as the written option. With something
  // picked the same text is a note about that choice. The server keeps the two
  // apart, so the mapping happens here rather than being guessed there.
  const commit = () => {
    const text = reply.value;
    if (picked.size > 0) {
      answer.selected = [...picked];
      answer.otherText = "";
      answer.notes = text;
    } else if (text.trim()) {
      answer.selected = [OTHER];
      answer.otherText = text;
      answer.notes = "";
    } else {
      answer.selected = [];
      answer.otherText = "";
      answer.notes = "";
    }
  };

  const render = () => {
    list.textContent = "";
    for (const option of entry.options) {
      const row = document.createElement("button");
      row.type = "button";
      row.className = "planenv-opt";
      if (picked.has(option.value)) row.classList.add("is-on");

      const box = document.createElement("i");
      box.className = `planenv-mark${entry.multiple ? "" : " is-one"}`;
      const label = document.createElement("span");
      label.className = "planenv-label";
      label.textContent = option.label;
      if (option.detail) {
        const hint = document.createElement("small");
        hint.textContent = option.detail;
        label.append(hint);
      }
      row.append(box, label);

      row.addEventListener("click", () => {
        const had = picked.has(option.value);
        if (!entry.multiple) picked.clear();
        if (had) picked.delete(option.value);
        else picked.add(option.value);
        change();
      });
      list.append(row);
    }
  };

  const change = () => {
    commit();
    render();
    paint(entry.key);
    save(card, entry.key);
  };

  reply.addEventListener("input", change);
  render();
  card.append(reply);

  const foot = document.createElement("footer");
  const status = document.createElement("span");
  status.className = "planenv-status";
  status.textContent = answer.answeredAt ? "answered" : "not answered";
  const clear = document.createElement("button");
  clear.type = "button";
  clear.className = "planenv-clear";
  clear.textContent = "clear";
  clear.addEventListener("click", () => {
    picked.clear();
    reply.value = "";
    change();
  });
  foot.append(status, clear);
  card.append(foot);
  return card;
};

let panel = null;
questions.forEach((entry, index) => {
  const ordinal = String(index + 1);
  const card = buildCard(entry, ordinal);
  cards.set(entry.key, card);

  // The agent marks the words a question is about with data-planenv-q. Only a
  // key this revision asks is decorated, so a marker left behind by an earlier
  // revision reads as ordinary prose rather than pointing at nothing.
  const marker = document.querySelector(`[data-planenv-q="${CSS.escape(entry.key)}"]`);
  if (marker) {
    marker.classList.add("planenv-marked");
    const number = document.createElement("sup");
    number.textContent = ordinal;
    marker.append(number);
    marker.addEventListener("click", () => reveal(entry.key));
    markers.set(entry.key, marker);
  }

  const anchor = entry.anchor ? document.getElementById(entry.anchor) : null;
  if (anchor) {
    // insert a sibling; never rewrite, reorder or restyle what the agent wrote
    anchor.after(card);
  } else {
    if (!panel) {
      panel = document.createElement("section");
      panel.id = "planenv-panel";
      panel.innerHTML = `<h2>Questions</h2>`;
      document.body.append(panel);
    }
    panel.append(card);
  }
  paint(entry.key);
});

for (const button of document.querySelectorAll(".planenv-step")) {
  button.addEventListener("click", () => step(Number(button.dataset.planenvStep)));
}

updateProgress();

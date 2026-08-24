#!/usr/bin/env node
// thegraph `search` node — justerm. Built from docs/agents/thegraph.md · thegraph stamp 2026-08-24.
//
// Routes a candidate by THE ARTIFACT IT TOUCHES — the module, the wire field, the predicate, the
// config key — never by the feature name: a related issue almost never shares your vocabulary.
//
// Usage:  node scripts/thegraph/search.mjs <artifact> [<artifact> …]
//
// THE TRIGGER IS NAMING, NOT DECIDING. Run this the moment you can say which artifact a candidate
// touches — usually while reading, before any probe. Ordering it after reproduction spends the
// expensive step first: a candidate an existing issue already owns gets reproduced and thrown away,
// and the tracker may well hold a BETTER measurement than the one you were about to take.
//
// ── WHAT THIS SCRIPT DOES NOT DO ────────────────────────────────────────────────────────────────
//
// It does not decide the CONFLICT out. "An existing issue whose proposal your change would break"
// is judgement, not counting — and justerm's measured cases are all semantic, never lexical: a wire
// channel claimed by two issues; one branch's entry condition, fg and bg filed as three independent
// decisions; one port capability surfacing as two symptoms. None of those shares vocabulary with the
// issue it conflicts with, so no query finds them. This script hands you the candidates; the
// adjudication stays on the main thread.
//
// It also does not file anything. Nothing reaches the tracker without passing `batch`.

import { spawnSync } from "node:child_process";

const REPO = "kihyun1998/justerm";

// ── the preemption table ─────────────────────────────────────────────────────────────────────────
//
// An area here ALREADY HAS the home a spine would provide, so a sibling is filed as a conformance
// item under that record and no anchor opens beside it. A *proposed* record counts: it is doing an
// anchor's job by construction (a hypothesis, a roster, an explicit not-yet-decided list).

const RECORDS = [
  { adr: "ADR-0017", area: "core ↔ consumer routing (mechanism vs policy)", match: /boundary|policy|mechanism|consumer|port/i },
  { adr: "ADR-0019", area: "renderer cell composition (a cell's bg / fg / ink)", match: /overlay|frame\.rs|decoration\.rs|glyph_class|composit|blend|ink/i },
  { adr: "ADR-0021", area: "renderer resource ownership / tiering", match: /surface|registry|tier|atlas|grid_id|viewport|context/i },
  { adr: "ADR-0024", area: "span projection / decoration geometry", match: /decorat|ruler|span|anchor|precedence/i },
  { adr: "ADR-0025", area: "row / wide-pair state ownership", match: /wrap|spacer|wide|pair|cell\b|row\b/i },
  { adr: "ADR-0026", area: "an out-of-range coordinate handed in from outside", match: /clamp|bound|out.of.range|pointer|column|coord/i },
  { adr: "ADR-0027", area: "when GPU work may be attempted", match: /context_loss|is_context_lost|restore|liveness|gpu_work/i },
  { adr: "ADR-0028", area: "what an IME composition puts on screen", match: /preedit|composition|ime/i },
  { adr: "ADR-0029", area: "when a coordinate leaves core", match: /marker|epoch|basis|tracked_point|command_mark/i },
  { adr: "0005/0008/0013–0016", area: "wire / frame shape", match: /serialize|encode|decode|wire|flat|VERSION/i },
];

// ADR-0022 (the cell is the ink box of `█`) and ADR-0023 (a spacing setting is CSS px) are the same
// SHAPE as 0021 and are deliberately NOT listed: nothing has re-decided them, and listing them would
// over-preempt anchors. A record earns a row here by having been re-litigated, not by existing.

// A map note does NOT preempt an anchor, however much it looks like one — it is descriptive and
// belongs in a file, while a roster is current state and belongs somewhere editable. The live case
// is docs/map/invariant/cell-size-is-derived-state.md with spine #630: both, deliberately.

const artifacts = process.argv.slice(2);
if (artifacts.length === 0) {
  console.error("usage: node scripts/thegraph/search.mjs <artifact> [<artifact> …]");
  console.error("       artifacts are modules, wire fields, predicates, config keys — not feature names");
  process.exit(2);
}

function gh(args) {
  const r = spawnSync("gh", args, { encoding: "utf8" });
  if (r.status !== 0) {
    console.error(`search: gh failed — ${(r.stderr || "").trim()}`);
    console.error("(gh fails outside a repo, and a redirect would leave an empty file — never pipe its output into --body-file)");
    process.exit(2);
  }
  return r.stdout;
}

console.log(`repo ${REPO}\n`);


for (const a of artifacts) {
  console.log(`▸ artifact: ${a}`);

  // No --state filter, deliberately: `gh search issues` accepts only open|closed, and omitting it
  // searches BOTH — which is what is wanted, because a CLOSED issue is the durable record of
  // REJECTED ALTERNATIVES and that record only pays off if it is read at the moment someone is
  // about to re-propose one.
  const out = gh(["search", "issues", "--repo", REPO, "--limit", "20", "--json", "number,title,state,url", a]);
  let rows = [];
  try {
    rows = JSON.parse(out);
  } catch {
    rows = [];
  }

  if (rows.length === 0) {
    console.log("  no issue mentions this artifact — an ordinary single issue, if it survives `batch`.\n");
  } else {
    for (const r of rows) console.log(`  #${r.number} [${r.state}] ${r.title}`);
    console.log("  ↳ READ what they already decided, INCLUDING what they rejected, before proposing a direction.");
    console.log("    A rejection may rest on a decision record, which then binds your direction and not just");
    console.log("    your filing location.\n");
  }

  const preempted = RECORDS.filter((r) => r.match.test(a));
  if (preempted.length) {
    console.log("  PREEMPTION — this area already carries a record, so a sibling here is a CONFORMANCE ITEM");
    console.log("  under it and NO anchor opens beside it:");
    for (const p of preempted) console.log(`    · ${p.adr} — ${p.area}`);
    console.log();
  }
}

console.log("──────── what this script did NOT answer ────────");
console.log("· CONFLICT: whether an existing issue's proposal your change would break. Semantic, not");
console.log("  lexical — no query finds it. Main thread. If you find one, cross-link BOTH ways in the");
console.log("  same act of filing and say which decision comes first.");
console.log("· Whether any of this should be filed at all. That is `batch`, and it is the maintainer's.");

// Always 0. This script never fails a run; it informs one — a non-zero would be a claim about the
// candidate, which is exactly what it declines to make. Finding nothing is a result, not a failure.
process.exit(0);
